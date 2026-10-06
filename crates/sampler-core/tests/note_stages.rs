use sampler_core::{Instruction as I, *};
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
fn plan(first: Vec<I>) -> Prepared {
    Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![
                Program::new(first).unwrap(),
                Program::new(vec![I::ForwardAttack]).unwrap(),
                Program::new(vec![])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_stages(vec![
            Stage {
                note: Some(0),
                ..Stage::default()
            },
            Stage::default(),
            Stage {
                note: Some(1),
                ..Stage::default()
            },
        ])
        .unwrap()
}
fn limits(plan: &Prepared, behaviors: usize) -> Limits {
    Limits {
        notes: 2,
        performances: 1,
        channels: 0,
        families: 0,
        voices: 0,
        expressions: 2,
        decisions: 0,
        commands: behaviors,
        behaviors,
        behavior_cells: plan.behavior_local_count() * behaviors,
        behavior_fuel: 64,
        note_cells: 0,
    }
}

#[test]
fn deferred_note_routes_reserve_capacity_and_retain_original_generation() {
    let prepared = plan(vec![I::Wait(4), I::ForwardAttack]);
    let budget = limits(&prepared, 2);
    let (mut rt, mut worker) = Runtime::with_plan_updates(prepared, budget, 2, 1).unwrap();
    let old = rt.active_plan();
    let note = support_note(&mut rt);
    worker
        .submit(Box::new(Prepared::new(48000, vec![], vec![], 0).unwrap()))
        .unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        rt.render(&mut [[0.; 2]; 5]).unwrap();
        assert_eq!(rt.note_plan(note), Ok(old));
        assert_eq!(
            rt.note_event_at(note, 2).unwrap().unwrap().pitch,
            NotePitch::Key(61)
        );
        assert_eq!(rt.note_pitch(note), Ok(NotePitch::Key(61)));
        assert_eq!(rt.note_off(input(1), None), Ok(note));
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    assert!(worker.retired().is_some());
}
fn support_note(rt: &mut Runtime) -> NoteId {
    let mut note = None;
    support::without_heap(|| {
        let first = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(rt.note_event_at(first, 2), Ok(None));
        assert_eq!(rt.trigger(input(2), 60, 1.), Err(Error::Capacity));
        assert_eq!(rt.note_count(), 1);
        rt.edit_note_event(
            first,
            NoteProperties {
                pitch: NotePitch::Key(61),
                velocity: 1.,
            },
        )
        .unwrap();
        note = Some(first);
    });
    note.unwrap()
}

#[test]
fn pending_note_reservations_return_on_suppression_fault_cancel_and_hard_stop() {
    for (code, action) in [
        (vec![I::SuppressAttack], 0),
        (
            vec![
                I::SetLocal {
                    local: 0,
                    value: -1,
                },
                I::WriteEventKey {
                    event: None,
                    local: 0,
                },
            ],
            0,
        ),
        (vec![I::Wait(20), I::ForwardAttack], 1),
        (vec![I::Wait(20), I::ForwardAttack], 2),
        (vec![I::Wait(20), I::ForwardAttack], 3),
    ] {
        let prepared = plan(code);
        let budget = limits(&prepared, 2);
        let mut rt = Runtime::new(prepared, budget).unwrap();
        support::without_heap(|| {
            let note = rt.trigger(input(1), 60, 1.).unwrap();
            match action {
                1 => {
                    rt.note_off(input(1), None).unwrap();
                }
                2 => rt.panic(),
                3 => {
                    rt.suppress_attack(note).unwrap();
                }
                _ => {}
            }
            assert_eq!(rt.note_event_at(note, 2), Ok(None));
            // The first callback still owns its retained outcome or wait. The
            // unreached downstream reservation must already be available.
            rt.start_plan_behavior(rt.active_plan(), 2).unwrap();
        });
    }
}

#[test]
fn long_note_routes_use_the_existing_bounded_dispatch_stack() {
    const COUNT: usize = 4096;
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(vec![Program::new(vec![I::ForwardAttack]).unwrap()], None)
        .unwrap()
        .with_stages(vec![
            Stage {
                note: Some(0),
                ..Stage::default()
            };
            COUNT
        ])
        .unwrap();
    let budget = limits(&prepared, COUNT);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            support::without_heap(|| {
                let note = rt.trigger(input(1), 60, 1.).unwrap();
                assert_eq!(
                    rt.note_event_at(note, COUNT).unwrap().unwrap().pitch,
                    NotePitch::Key(60)
                );
                let mut completed = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    completed += 1;
                    true
                });
                assert_eq!(completed, COUNT);
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn held_release_routes_keep_and_return_their_remaining_callback_capacity() {
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![I::SuppressRelease, I::Wait(20)])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
                Program::new(vec![I::ForwardReleaseGroups])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
                Program::new(vec![])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_stages(vec![
            Stage {
                release: Some(0),
                ..Stage::default()
            },
            Stage::default(),
            Stage {
                release: Some(1),
                ..Stage::default()
            },
        ])
        .unwrap();
    let budget = limits(&prepared, 2);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(rt.trigger(input(2), 60, 1.), Err(Error::Capacity));
        assert_eq!(rt.note_count(), 1);
        rt.note_off(input(1), None).unwrap();
        assert!(rt.release_context(note).unwrap().gate.is_none());
        assert_eq!(
            rt.start_plan_behavior(rt.active_plan(), 2),
            Err(Error::Capacity)
        );
        rt.panic();
        assert_eq!(rt.pending_commands(), 0);
        rt.start_plan_behavior(rt.active_plan(), 2).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            assert!(matches!(outcome, Outcome::Cancelled | Outcome::Finished));
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
        // Every unstarted release slot was returned before acknowledgement.
        rt.trigger(input(2), 60, 1.).unwrap();
    });
}

#[test]
fn long_release_routes_complete_without_recursive_calls_or_extra_vm_frames() {
    const COUNT: usize = 4096;
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![I::ForwardReleaseGroups])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_stages(vec![
            Stage {
                release: Some(0),
                ..Stage::default()
            };
            COUNT
        ])
        .unwrap();
    let budget = limits(&prepared, COUNT);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            support::without_heap(|| {
                let note = rt.trigger(input(1), 60, 1.).unwrap();
                rt.note_off(input(1), None).unwrap();
                assert!(rt.release_context(note).unwrap().gate.is_some());
                let mut completed = 0;
                rt.flush_behaviors(|_, _, outcome| {
                    assert_eq!(outcome, Outcome::Finished);
                    completed += 1;
                    true
                });
                assert_eq!(completed, COUNT);
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn native_release_completion_does_not_implicitly_forward_a_frontend_event() {
    let prepared = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
                Program::new(vec![I::ForwardReleaseGroups])
                    .unwrap()
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap()
        .with_stages(vec![
            Stage {
                release: Some(0),
                ..Stage::default()
            },
            Stage {
                release: Some(1),
                ..Stage::default()
            },
        ])
        .unwrap();
    let budget = limits(&prepared, 2);
    let mut rt = Runtime::new(prepared, budget).unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        rt.note_off(input(1), None).unwrap();
        assert!(rt.release_context(note).unwrap().gate.is_none());
        assert!(rt.forward_release_groups(note).unwrap());
        assert!(rt.release_context(note).unwrap().gate.is_some());
        assert!(!rt.forward_release_groups(note).unwrap());
        let mut completed = 0;
        rt.flush_behaviors(|_, _, outcome| {
            assert_eq!(outcome, Outcome::Finished);
            completed += 1;
            true
        });
        assert_eq!(completed, 2);
    });
}
