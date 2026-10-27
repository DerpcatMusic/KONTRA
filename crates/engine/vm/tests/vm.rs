#![cfg(test)]

use kontra_vm::{Cmp, HostApi, HostCall, HostValue, Instance, Op, Program, Returns, Step, Text};

/// Records host calls; id 1 returns the sum of its ints, id 2 echoes its text.
#[derive(Default)]
struct Host {
    calls: Vec<(u16, u64, Vec<i64>, Vec<String>)>,
}

impl HostApi for Host {
    fn call(&mut self, call: HostCall<'_>) -> HostValue {
        self.calls.push((
            call.id,
            call.context,
            call.ints.to_vec(),
            call.texts.iter().map(|t| t.as_str().to_owned()).collect(),
        ));
        match call.id {
            1 => HostValue::Int(call.ints.iter().sum::<i64>() as i32),
            2 => match call.texts.first() {
                Some(t) => {
                    *call.text_out = *t;
                    HostValue::Text
                }
                None => HostValue::Nothing,
            },
            _ => HostValue::Nothing,
        }
    }
}

fn program(code: Vec<Op>) -> Program {
    Program {
        code,
        cells: 16,
        text_cells: 4,
        poly_cells: 2,
        max_depth: 8,
        max_stack: 32,
        ..Program::default()
    }
}

fn run(p: &Program, host: &mut Host) -> (Instance, Step) {
    let mut vm = Instance::new(p, 4);
    let co = vm.start(0, 7).unwrap();
    let step = vm.run(p, co, host);
    (vm, step)
}

#[test]
fn a_while_loop_sums_and_wraps_at_32_bits() {
    // cell0 = 0; cell1 = 0; while cell0 < 10 { cell1 += cell0; cell0 += 1 }; cell2 = MAX + 1
    let p = program(vec![
        Op::Int(0),
        Op::Store(0),
        Op::Int(0),
        Op::Store(1),               // 0..3
        Op::LoopCheck { exit: 18 }, // 4
        Op::Load(0),
        Op::Int(10),
        Op::ICmp(Cmp::Lt),
        Op::JumpIfZero(18),
        Op::Load(1),
        Op::Load(0),
        Op::IAdd,
        Op::Store(1), // 9..12
        Op::Load(0),
        Op::Int(1),
        Op::IAdd,
        Op::Store(0), // 13..16
        Op::Jump(4),  // 17
        Op::LoopExit, // 18
        Op::Int(i32::MAX),
        Op::Int(1),
        Op::IAdd,
        Op::Store(2),
        Op::Exit,
    ]);
    let (vm, step) = run(&p, &mut Host::default());
    assert_eq!(step, Step::Done);
    assert_eq!(vm.cell(1), Some(45));
    assert_eq!(vm.cell(2), Some(i64::from(i32::MIN)), "wraps like KSP");
}

#[test]
fn a_loop_that_cannot_exit_leaves_early_and_the_callback_continues() {
    let p = program(vec![
        Op::LoopCheck { exit: 2 }, // 0
        Op::Jump(0),               // 1
        Op::LoopExit,              // 2
        Op::Int(5),
        Op::Store(0),
        Op::Exit,
    ]);
    let t = std::time::Instant::now();
    let (vm, step) = run(&p, &mut Host::default());
    assert_eq!(step, Step::Done);
    assert_eq!(vm.cell(0), Some(5));
    assert_eq!(vm.faults().loop_limits, 1);
    assert!(
        t.elapsed().as_millis() < 50,
        "left at the first stuck check, not after 10M turns"
    );
}

#[test]
fn wait_suspends_and_resume_continues_where_it_left() {
    let p = program(vec![
        Op::Int(1),
        Op::Store(0),
        Op::Int(2_000),
        Op::Wait,
        Op::Int(2),
        Op::Store(0),
        Op::Exit,
    ]);
    let mut vm = Instance::new(&p, 2);
    let mut host = Host::default();
    let co = vm.start(0, 0).unwrap();
    assert_eq!(vm.run(&p, co, &mut host), Step::Wait(2_000));
    assert_eq!(vm.cell(0), Some(1));
    assert_eq!(vm.busy(), 1);
    assert_eq!(vm.run(&p, co, &mut host), Step::Done);
    assert_eq!(vm.cell(0), Some(2));
    assert_eq!(vm.busy(), 0);
}

#[test]
fn host_calls_get_their_arguments_and_context_and_return_values() {
    let p = program(vec![
        Op::Int(3),
        Op::Int(4),
        Op::Host {
            id: 1,
            ints: 2,
            texts: 0,
            returns: Returns::Int,
        },
        Op::Store(0),
        Op::Text(0),
        Op::Int(42),
        Op::TFromInt,
        Op::TConcat,
        Op::Host {
            id: 2,
            ints: 0,
            texts: 1,
            returns: Returns::Text,
        },
        Op::TStore(0),
        Op::Exit,
    ]);
    let mut p = p;
    p.texts = vec![Text::from_str_cut("v=").0];
    let mut host = Host::default();
    let (vm, _) = run(&p, &mut host);
    assert_eq!(vm.cell(0), Some(7));
    assert_eq!(vm.text(0).map(Text::as_str), Some("v=42"));
    assert_eq!(host.calls[0], (1, 7, vec![3, 4], vec![]));
}

#[test]
fn out_of_range_and_divide_by_zero_are_counted_not_fatal() {
    let p = program(vec![
        Op::Int(9),
        Op::LoadAt { base: 0, len: 4 },
        Op::Store(5),
        Op::Int(-1),
        Op::Int(1),
        Op::StoreAt { base: 0, len: 4 },
        Op::Int(7),
        Op::Int(0),
        Op::IDiv,
        Op::Store(6),
        Op::Int(2),
        Op::Int(11),
        Op::StoreAt { base: 0, len: 4 },
        Op::Exit,
    ]);
    let (vm, step) = run(&p, &mut Host::default());
    assert_eq!(step, Step::Done);
    assert_eq!(
        (vm.faults().out_of_range, vm.faults().divide_by_zero),
        (2, 1)
    );
    assert_eq!(vm.cell(2), Some(11), "the in-range write landed");
}

#[test]
fn calls_return_and_exit_ends_from_any_depth() {
    let p = program(vec![
        Op::Call(4),
        Op::Int(1),
        Op::Store(0),
        Op::Exit, // 0..3
        Op::Call(7),
        Op::Int(2),
        Op::Store(1), // 4..6: never reaches 5
        Op::Exit,     // 7: function body
    ]);
    let (vm, step) = run(&p, &mut Host::default());
    assert_eq!(step, Step::Done);
    assert_eq!((vm.cell(0), vm.cell(1)), (Some(0), Some(0)));
}

#[test]
fn polyphonic_cells_belong_to_each_callback() {
    let p = program(vec![
        Op::Host {
            id: 9,
            ints: 0,
            texts: 0,
            returns: Returns::Nothing,
        },
        Op::LoadPoly(0),
        Op::Int(1),
        Op::IAdd,
        Op::Dup,
        Op::StorePoly(0),
        Op::Store(0),
        Op::Int(10),
        Op::Wait,
        Op::LoadPoly(0),
        Op::Store(1),
        Op::Exit,
    ]);
    let mut vm = Instance::new(&p, 2);
    let mut host = Host::default();
    let a = vm.start(0, 1).unwrap();
    let b = vm.start(0, 2).unwrap();
    assert_eq!(vm.run(&p, a, &mut host), Step::Wait(10));
    assert_eq!(vm.run(&p, b, &mut host), Step::Wait(10));
    assert_eq!(
        vm.cell(0),
        Some(1),
        "each callback has its own polyphonic cell"
    );
    assert!(vm.start(0, 3).is_none() && vm.faults().no_coroutine == 1);
    vm.run(&p, a, &mut host);
    assert_eq!(vm.cell(1), Some(1));
}

#[test]
fn reals_compute_and_convert() {
    let p = program(vec![
        Op::Real(1.5),
        Op::Int(2),
        Op::IntToReal,
        Op::FMul,
        Op::RealToInt,
        Op::Store(0),
        Op::Real(f64::NAN),
        Op::Real(1.0),
        Op::FCmp(Cmp::Ne),
        Op::Store(1),
        Op::Exit,
    ]);
    let (vm, _) = run(&p, &mut Host::default());
    assert_eq!(vm.cell(0), Some(3));
    assert_eq!(vm.cell(1), Some(1));
}
