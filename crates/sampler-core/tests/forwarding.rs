use sampler_core::{
    Envelope, Error, Expression, Input, Limits, Pcm, Playback, Prepared, Protocol, Region,
    ReleaseOptions, ReleaseReserve, Runtime, Trigger,
};
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
        channels: 1,
        performances: 1,
        families: 2,
        expressions: 4,
        voices: 2,
        decisions: 0,
        commands: 0,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}
fn plan(value: f32) -> Prepared {
    let region = Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    Prepared::new(
        48000,
        vec![
            Pcm::new(48000, Box::from([[value; 2]; 4])).unwrap(),
            Pcm::new(48000, Box::from([[-0.5 * value; 2]; 2])).unwrap(),
        ],
        vec![
            region,
            Region {
                sample: 1,
                ..region
            },
        ],
        2,
    )
    .unwrap()
    .with_releases(
        vec![Trigger::Attack, Trigger::GateRelease],
        ReleaseOptions::default(),
        ReleaseOptions::default(),
    )
    .unwrap()
}
#[test]
fn forwarding_is_once_per_original_note_and_failed_capacity_is_retryable_without_heap() {
    let mut rt = Runtime::new(plan(1.), limits()).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(1), 60, 1.).unwrap();
        let b = rt.note_on(input(2), 60, 1.).unwrap();
        assert!(rt.forward_attack(a).unwrap());
        assert!(!rt.forward_attack(a).unwrap());
        assert!(!rt.suppress_attack(a).unwrap());
        let reserve = rt.release_reserve();
        assert_eq!(reserve.voices, 1);
        assert_eq!(rt.forward_attack(b), Err(Error::Capacity));
        assert_eq!(rt.release_reserve(), reserve);
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.family_count()),
            (2, 1, 1)
        );
        rt.key_up(a, None).unwrap();
        let mut release = [[0.; 2]; 2];
        rt.render(&mut release).unwrap();
        assert_eq!(release, [[-0.5; 2]; 2]);
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 1);
        assert!(rt.forward_attack(b).unwrap());
        assert_eq!(rt.release_reserve(), reserve);
        assert_eq!(
            rt.note_count(),
            1,
            "forwarding must never create a child identity"
        );
        rt.key_up(b, None).unwrap();
        rt.render(&mut release).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.release_reserve(), ReleaseReserve::default());
        assert_eq!(rt.forward_attack(a), Err(Error::StaleHandle));
        let ignored = rt.note_on(input(3), 60, 1.).unwrap();
        assert!(rt.suppress_attack(ignored).unwrap());
        assert!(!rt.suppress_attack(ignored).unwrap());
        assert!(!rt.forward_attack(ignored).unwrap());
        rt.key_up(ignored, None).unwrap();
        assert!(!rt.forward_attack(ignored).unwrap());
        rt.render(&mut release).unwrap();
        assert_eq!(release, [[0.; 2]; 2]);
        rt.flush_ended(|_| true);
        let closed = rt.note_on(input(4), 60, 1.).unwrap();
        rt.key_up(closed, None).unwrap();
        assert_eq!(rt.forward_attack(closed), Err(Error::ClosedNote));
        rt.flush_ended(|_| true);
        let channel = rt.register_channel(input(5).channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let held = rt.note_on(input(5), 60, 1.).unwrap();
        rt.key_up(held, None).unwrap();
        assert!(rt.note(held).unwrap().2);
        assert_eq!(rt.forward_attack(held), Err(Error::ClosedNote));
        assert_eq!(rt.release_reserve(), ReleaseReserve::default());
        rt.sustain(channel, false).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.family_count()),
            (0, 0, 0)
        );
    });
    let prepared = plan(1.)
        .with_programs(
            vec![
                sampler_core::Program::new(vec![sampler_core::Instruction::ForwardAttack]).unwrap(),
            ],
            Some(0),
        )
        .unwrap();
    let mut rt = Runtime::new(
        prepared,
        Limits {
            voices: 1,
            behaviors: 1,
            behavior_fuel: 2,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(5), 60, 1.).unwrap();
        assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
        assert_eq!(rt.release_reserve(), ReleaseReserve::default());
        rt.flush_ended(|_| panic!("fault outcome still owns original note"));
        rt.flush_behaviors(|_, owner, outcome| {
            assert_eq!(owner, sampler_core::BehaviorOwner::Note(note));
            assert_eq!(outcome, sampler_core::Outcome::Fault(Error::Capacity));
            true
        });
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn deferred_mapping_keeps_original_plan_full_resolution_and_live_expression_owner() {
    let (mut rt, mut control) = Runtime::with_plan_updates(plan(1.), limits(), 2, 1).unwrap();
    control.submit(Box::new(plan(0.5))).unwrap();
    support::without_heap(|| {
        let old_plan = rt.active_plan();
        let velocity = 0.123456789123;
        let note = rt
            .note_on_with_expression(
                input(1),
                60,
                velocity,
                Expression {
                    gain: 0.25,
                    ..Expression::default()
                },
            )
            .unwrap();
        let owner = rt.expression_id(note).unwrap();
        assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
        assert!(rt.forward_attack(note).unwrap());
        assert_eq!(rt.note_plan(note), Ok(old_plan));
        assert_eq!(rt.note(note).unwrap().1, velocity);
        assert_eq!(rt.expression_id(note), Ok(owner));
        let mut audio = [[0.; 2]];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[velocity as f32 * 0.25; 2]]);
        rt.set_expression(
            owner,
            Expression {
                gain: 0.5,
                ..Expression::default()
            },
        )
        .unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[velocity as f32 * 0.5; 2]]);
        rt.panic();
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(rt.note_count(), 0);
    });
    drop(control.retired().unwrap());
}
