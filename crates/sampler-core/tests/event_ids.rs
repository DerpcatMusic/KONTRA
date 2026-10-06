use sampler_core::*;
mod support;
fn limits(notes: usize) -> Limits {
    Limits {
        notes,
        channels: 0,
        performances: 1,
        expressions: notes,
        families: notes,
        voices: notes,
        decisions: 0,
        commands: notes,
        behaviors: notes,
        behavior_fuel: 64,
        behavior_cells: notes * 4,
        note_cells: 0,
    }
}
fn plan() -> Prepared {
    Prepared::new(48000, vec![], vec![], 0).unwrap()
}
fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(99),
    }
}
#[test]
fn source_ids_are_not_host_ids_and_never_alias_reused_note_slots_or_panic() {
    let mut rt = Runtime::new(plan(), limits(1)).unwrap();
    support::without_heap(|| {
        let plan = rt.active_plan();
        let mut previous = 0;
        for _ in 0..300 {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let id = rt.source_event_id(note).unwrap();
            assert!(id > previous);
            assert_eq!(rt.source_event_id(note), Ok(id));
            assert_eq!(rt.resolve_source_event(plan, id), Ok(Some(note)));
            assert_eq!(rt.resolve_source_event(plan, previous), Ok(None));
            for invalid in [0, -1, i32::MIN, i32::MAX] {
                assert_eq!(rt.resolve_source_event(plan, invalid), Ok(None));
            }
            rt.panic();
            rt.flush_ended(|_| false);
            assert_eq!(rt.resolve_source_event(plan, id), Ok(Some(note))); // Terminal owner still retained.
            rt.flush_ended(|_| true);
            assert_eq!(rt.resolve_source_event(plan, id), Ok(None));
            assert_eq!(rt.source_event_id(note), Err(Error::StaleHandle));
            previous = id;
        }
    });
}
#[test]
fn source_id_lookup_requires_the_original_generation_and_runtime() {
    let (mut rt, mut control) = Runtime::with_plan_updates(plan(), limits(4), 2, 1).unwrap();
    let mut other = Runtime::new(plan(), limits(1)).unwrap();
    control.submit(Box::new(plan())).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(), 60, 1.).unwrap();
        let id = rt.source_event_id(a).unwrap();
        let old = rt.active_plan();
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_eq!(rt.resolve_source_event(old, id), Ok(Some(a)));
        assert_eq!(rt.resolve_source_event(new, id), Ok(None));
        let child = rt
            .child(a, 61, 1., false, Inheritance::Independent)
            .unwrap();
        let child_id = rt.source_event_id(child).unwrap();
        let b = rt
            .note_on(
                Input {
                    external_id: Some(100),
                    ..input()
                },
                60,
                1.,
            )
            .unwrap();
        let b_id = rt.source_event_id(b).unwrap();
        assert_eq!(rt.resolve_source_event(old, child_id), Ok(Some(child)));
        assert_eq!(rt.resolve_source_event(new, child_id), Ok(None));
        assert_eq!(rt.resolve_source_event(old, b_id), Ok(None));
        assert_eq!(rt.resolve_source_event(new, b_id), Ok(Some(b)));
        assert_eq!(other.resolve_source_event(old, id), Err(Error::StaleHandle));
        let foreign = other.note_on(input(), 60, 1.).unwrap();
        assert_eq!(rt.source_event_id(foreign), Err(Error::StaleHandle));
        rt.panic();
        rt.flush_ended(|_| true);
        rt.collect_retired_plans();
        assert_eq!(rt.resolve_source_event(old, id), Err(Error::StaleHandle));
        assert_eq!(rt.resolve_source_event(new, b_id), Ok(None));
    });
    drop(control.retired().unwrap());
}

#[test]
fn source_id_exhaustion_is_explicit_and_does_not_exhaust_native_note_ownership() {
    assert!(plan().with_source_event_limit(0).is_err());
    assert!(plan().with_source_event_limit(-1).is_err());
    let mut rt = Runtime::new(plan().with_source_event_limit(2).unwrap(), limits(2)).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(), 60, 1.).unwrap();
        let first = rt.source_event_id(a).unwrap();
        let b = rt
            .child(a, 60, 1., false, Inheritance::Independent)
            .unwrap();
        let last = rt.source_event_id(b).unwrap();
        assert_eq!((first, last), (1, 2));
        rt.key_up(b, None).unwrap();
        rt.flush_ended(|_| true);
        let c = rt
            .child(a, 60, 1., false, Inheritance::Independent)
            .unwrap();
        assert_eq!(rt.source_event_id(c), Err(Error::Capacity));
        assert_eq!(rt.source_event_id(a), Ok(first));
        assert_eq!(rt.resolve_source_event(rt.active_plan(), last), Ok(None));
        assert_eq!(rt.note_count(), 2);
        rt.panic();
        rt.flush_ended(|_| true);
        let d = rt.note_on(input(), 60, 1.).unwrap();
        assert_eq!(rt.source_event_id(d), Err(Error::Capacity));
        assert_eq!(rt.resolve_source_event(rt.active_plan(), first), Ok(None));
    });
}

#[test]
fn generated_result_aliases_are_preflighted_and_do_not_pin_completed_children() {
    let program = || {
        Program::new(vec![
            Instruction::SetLocal {
                local: 0,
                value: 60,
            },
            Instruction::SetLocal {
                local: 1,
                value: 127,
            },
            Instruction::PlayMidi {
                key: 0,
                velocity: 1,
                duration: DurationValue::Fixed(Duration::UntilSilent),
                inheritance: Inheritance::Independent,
                result: Some(0),
            },
            Instruction::ReadEventId { local: 1 },
            Instruction::End,
        ])
        .unwrap()
    };
    let make = |maximum| {
        plan()
            .with_programs(vec![program()], None)
            .unwrap()
            .with_source_event_limit(maximum)
            .unwrap()
    };
    let mut rt = Runtime::new(make(1), limits(3)).unwrap();
    support::without_heap(|| {
        let n = rt.note_on(input(), 60, 1.).unwrap();
        rt.source_event_id(n).unwrap();
        let callback = rt.start_behavior(n, 0).unwrap();
        assert_eq!(
            rt.behavior_outcome(callback),
            Ok(Some(Outcome::Fault(Error::Capacity)))
        );
        assert_eq!(rt.behavior_local(callback, 0), Ok(60));
        assert_eq!(
            (
                rt.note_count(),
                rt.expression_count(),
                rt.voice_count(),
                rt.pending_commands()
            ),
            (1, 1, 0, 0)
        );
    });
    let mut rt = Runtime::new(make(10), limits(3)).unwrap();
    support::without_heap(|| {
        let n = rt.note_on(input(), 60, 1.).unwrap();
        let callback = rt.start_behavior(n, 0).unwrap();
        assert_eq!(rt.behavior_outcome(callback), Ok(Some(Outcome::Finished)));
        let child_id = rt.behavior_local(callback, 0).unwrap() as i32;
        let parent_id = rt.behavior_local(callback, 1).unwrap() as i32;
        assert_ne!(child_id, parent_id);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), parent_id),
            Ok(Some(n))
        );
        assert!(
            rt.resolve_source_event(rt.active_plan(), child_id)
                .unwrap()
                .is_some()
        );
        rt.flush_ended(|_| false);
        assert_eq!(
            rt.resolve_source_event(rt.active_plan(), child_id),
            Ok(None)
        );
        assert_eq!(rt.note_count(), 1);
    });
    let program = Program::new(vec![
        Instruction::End,
        Instruction::ReadEventId { local: 4 },
    ])
    .unwrap();
    assert!(program.requires_note());
    assert!(matches!(
        Runtime::new(
            plan().with_programs(vec![program], None).unwrap(),
            limits(1)
        ),
        Err(Error::Capacity)
    ));
}
