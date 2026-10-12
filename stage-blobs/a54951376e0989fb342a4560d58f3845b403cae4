use sampler_core::*;
mod support;

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        expressions: 4,
        families: 4,
        voices: 4,
        decisions: 0,
        commands: 8,
        behaviors: 8,
        behavior_fuel: 64,
        behavior_cells: 16,
        note_cells: 20,
    }
}
fn writer(instance: Option<u16>, cell: u16, offset: i64) -> Program {
    let program = Program::new(vec![
        Instruction::ReadKey { local: 0 },
        Instruction::AddLocal {
            local: 0,
            value: offset,
        },
        Instruction::WriteNoteCell { cell, local: 0 },
        Instruction::Wait(4),
        Instruction::ReadNoteCell { cell, local: 1 },
    ])
    .unwrap()
    .with_wait_lifetime(WaitLifetime::Callback);
    match instance {
        Some(id) => program.with_script_instance(ScriptInstanceId(id)),
        None => program,
    }
}
fn plan(programs: Vec<Program>) -> Prepared {
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![], vec![]])
        .unwrap()
        .with_programs(programs, None)
        .unwrap()
}

#[test]
fn independent_instances_share_a_note_without_sharing_polyphonic_cells() {
    let prepared = plan(vec![
        writer(Some(0), 0, 0),
        writer(Some(1), 1, 100),
        writer(None, 0, 200),
        writer(Some(0), 1, 300),
        Program::new(vec![Instruction::ReadNoteCell { cell: 0, local: 0 }])
            .unwrap()
            .with_script_instance(ScriptInstanceId(0)),
    ]);
    assert_eq!(prepared.note_cell_count(), 5); // max per instance, then sum.
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(0), 60, 1.).unwrap();
        let b = rt.note_on(input(1), 61, 1.).unwrap();
        let callbacks: [_; 4] = std::array::from_fn(|p| rt.start_behavior(a, p).unwrap());
        let other = rt.start_behavior(b, 0).unwrap();
        let reader = rt.start_behavior(a, 4).unwrap();
        assert_eq!(rt.behavior_local(reader, 0), Ok(60));
        assert_eq!(rt.program_note_cell(a, 0, 0), Ok(60));
        assert_eq!(rt.program_note_cell(a, 1, 1), Ok(160));
        assert_eq!(rt.program_note_cell(a, 1, 0), Ok(0));
        assert_eq!(rt.program_note_cell(a, 2, 0), Ok(260));
        assert_eq!(rt.program_note_cell(a, 3, 1), Ok(360));
        assert_eq!(rt.program_note_cell(a, 0, 1), Err(Error::InvalidInput));
        assert_eq!(rt.program_note_cell(a, 99, 0), Err(Error::InvalidInput));
        assert_eq!(
            rt.start_plan_behavior(rt.active_plan(), 4),
            Err(Error::InvalidInput)
        );
        rt.key_up(a, None).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        for (callback, value) in callbacks.into_iter().zip([60, 160, 260, 360]) {
            assert_eq!(rt.behavior_local(callback, 1), Ok(value));
            assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
        }
        assert_eq!(rt.behavior_local(other, 1), Ok(61));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        let reused = rt.note_on(input(0), 62, 1.).unwrap();
        for p in 0..5 {
            assert_eq!(rt.program_note_cell(reused, p, 0), Ok(0));
        }
        assert_eq!(rt.program_note_cell(a, 0, 0), Err(Error::StaleHandle));
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn instance_layouts_follow_generations_and_capacity_counts_the_sum_of_namespaces() {
    let initial = || plan(vec![writer(Some(0), 2, 0), writer(Some(1), 1, 100)]);
    assert!(matches!(
        Runtime::new(
            initial(),
            Limits {
                note_cells: 16,
                ..limits()
            }
        ),
        Err(Error::Capacity)
    ));
    let (mut rt, mut transfer) = Runtime::with_plan_updates(initial(), limits(), 2, 1).unwrap();
    let rejected = Box::new(plan(vec![writer(Some(0), 3, 0), writer(Some(1), 1, 0)]));
    let address = std::ptr::from_ref(rejected.as_ref());
    let rejected = transfer.submit(rejected).unwrap_err();
    assert_eq!(rejected.reason, PlanError::NoteStateCapacity);
    assert_eq!(std::ptr::from_ref(rejected.prepared.as_ref()), address);
    transfer
        .submit(Box::new(plan(vec![
            writer(Some(1), 0, 200),
            writer(Some(0), 0, 300),
        ])))
        .unwrap();
    support::without_heap(|| {
        let old = rt.note_on(input(0), 60, 1.).unwrap();
        let old_callbacks = [
            rt.start_behavior(old, 0).unwrap(),
            rt.start_behavior(old, 1).unwrap(),
        ];
        rt.poll_plan_update().unwrap();
        let fresh = rt.note_on(input(1), 61, 1.).unwrap();
        let fresh_callbacks = [
            rt.start_behavior(fresh, 0).unwrap(),
            rt.start_behavior(fresh, 1).unwrap(),
        ];
        let child = rt.child(old, 62, 1., false, Inheritance::Linked).unwrap();
        assert_eq!(rt.program_note_cell(child, 0, 2), Ok(0));
        assert_eq!(rt.program_note_cell(child, 1, 1), Ok(0));
        assert_eq!(rt.program_note_cell(fresh, 0, 2), Err(Error::InvalidInput));
        rt.key_up(old, None).unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        for (callback, value) in old_callbacks
            .into_iter()
            .chain(fresh_callbacks)
            .zip([60, 160, 261, 361])
        {
            assert_eq!(rt.behavior_local(callback, 1), Ok(value));
        }
        rt.panic();
        rt.flush_behaviors(|_, _, _| false);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(rt.note_count(), 0);
    });
    drop(transfer.retired().unwrap());
    // The highest source cell remains per-instance, not one global u16 address.
    let wide = plan(vec![
        writer(Some(0), u16::MAX, 0),
        writer(Some(1), u16::MAX, 1),
    ]);
    assert_eq!(wide.note_cell_count(), 2 * (usize::from(u16::MAX) + 1));
}
