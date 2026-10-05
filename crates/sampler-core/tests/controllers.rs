use sampler_core::{
    Error, Event, Expression, Inheritance, Input, Limits, NotePitch, Prepared, Protocol, Runtime,
};
mod support;

fn limits() -> Limits {
    Limits {
        notes: 8,
        performances: 3,
        channels: 0,
        voices: 0,
        families: 0,
        expressions: 8,
        decisions: 0,
        commands: 2,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
    }
}
fn runtime() -> Runtime {
    Runtime::new(Prepared::new(48000, vec![], vec![], 0).unwrap(), limits()).unwrap()
}
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

#[test]
fn full_note_capacity_and_continuous_changes_keep_snapshots_without_heap_work() {
    let mut rt = runtime();
    support::without_heap(|| {
        for cycle in 0..100u32 {
            let mut notes = [None; 8];
            for (i, slot) in notes.iter_mut().enumerate() {
                let domain = rt.performance(i % 3).unwrap();
                let value = cycle * 1000 + i as u32;
                rt.set_articulation(domain, value).unwrap();
                for cc in 0..128 {
                    rt.set_controller(domain, cc, value + u32::from(cc))
                        .unwrap();
                }
                *slot = Some(
                    rt.note_on_pitched_in(
                        domain,
                        input(i as i32),
                        NotePitch::Key(60),
                        1.,
                        Expression::default(),
                    )
                    .unwrap(),
                );
            }
            // Failed admission must not acquire a version owner.
            assert_eq!(rt.note_on(input(999), 60, 1.), Err(Error::Capacity));
            for step in 0..512 {
                let domain = rt.performance(step % 3).unwrap();
                rt.set_controller(domain, 1, u32::MAX - step as u32)
                    .unwrap();
                rt.set_articulation(domain, u32::MAX).unwrap();
            }
            for (i, note) in notes.into_iter().enumerate() {
                let note = note.unwrap();
                let value = cycle * 1000 + i as u32;
                assert_eq!(rt.note_selection(note).unwrap().articulation, value);
                for cc in 0..128 {
                    assert_eq!(rt.note_controller(note, cc).unwrap(), value + u32::from(cc));
                }
                rt.key_up(note, None).unwrap();
                rt.flush_ended(|_| false);
                assert_eq!(rt.note_controller(note, 1).unwrap(), value + 1);
            }
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
            assert_eq!(
                rt.note_controller(notes[0].unwrap(), 0),
                Err(Error::StaleHandle)
            );
        }
    });
}

#[test]
fn controller_timeline_preserves_boundaries_precision_and_rejection_atomicity() {
    let mut rt = runtime();
    let other = runtime();
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        let foreign = other.performance(1).unwrap();
        assert_eq!(rt.set_controller(foreign, 1, 5), Err(Error::StaleHandle));
        assert_eq!(rt.set_controller(domain, 128, 5), Err(Error::InvalidInput));
        assert_eq!(rt.controller(domain, 255), Err(Error::InvalidInput));
        rt.schedule_event(2, Event::Controller(domain, 1, 0x12345678))
            .unwrap();
        rt.schedule_event(2, Event::Controller(domain, 1, 0x12345679))
            .unwrap();
        assert_eq!(
            rt.schedule_event(3, Event::Controller(domain, 1, 42)),
            Err(Error::Capacity)
        );
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        assert_eq!(rt.controller(domain, 1).unwrap(), 0);
        rt.render(&mut []).unwrap();
        assert_eq!(rt.controller(domain, 1).unwrap(), 0x12345679);
        let note = rt
            .note_on_pitched_in(
                domain,
                input(0),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        assert_eq!(rt.note_controller(note, 1).unwrap(), 0x12345679);
        rt.set_controller(domain, 1, 0x1234567a).unwrap();
        assert_eq!(rt.note_controller(note, 1).unwrap(), 0x12345679);
        assert_eq!(rt.controller(rt.performance(0).unwrap(), 1).unwrap(), 0);
        assert_eq!(
            rt.schedule_event(1, Event::Controller(domain, 1, 42)),
            Err(Error::PastEvent)
        );
        rt.schedule_event(3, Event::Controller(domain, 1, 42))
            .unwrap();
        rt.panic();
        rt.render(&mut [[0.; 2]; 4]).unwrap();
        assert_eq!(rt.controller(domain, 1).unwrap(), 0x1234567a);
        rt.flush_ended(|_| true);
    });
}

#[test]
fn children_capture_current_domain_controllers_independently_of_expression_inheritance() {
    let mut rt = runtime();
    support::without_heap(|| {
        let domain = rt.performance(2).unwrap();
        rt.set_controller(domain, 7, 17).unwrap();
        let parent = rt
            .note_on_pitched_in(
                domain,
                input(0),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        for (i, inheritance) in [
            Inheritance::Independent,
            Inheritance::Snapshot,
            Inheritance::Linked,
        ]
        .into_iter()
        .enumerate()
        {
            let value = 100 + i as u32;
            rt.set_controller(domain, 7, value).unwrap();
            let child = rt.child(parent, 60, 1., true, inheritance).unwrap();
            assert_eq!(rt.note_controller(child, 7).unwrap(), value);
            assert_eq!(rt.note_controller(parent, 7).unwrap(), 17);
            rt.pin(child).unwrap();
            rt.release(child).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_controller(child, 7).unwrap(), value);
            rt.unpin(child).unwrap();
        }
        rt.key_up(parent, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}
