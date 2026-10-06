use sampler_core::{
    Envelope, Error, Expression, Inheritance, Input, Limits, Pcm, Playback, Prepared, Protocol,
    Region, ReleaseOptions, Runtime, Trigger,
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
        notes: 8,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 8,
        voices: 32,
        decisions: 8,
        commands: 8,
        behaviors: 4,
        behavior_fuel: 1024,
        behavior_cells: 16,
        note_cells: 0,
    }
}
fn plan() -> Prepared {
    let pcm = [0.125, 0.25, 0.5, -0.125, -0.25, -0.5]
        .map(|value| Pcm::new(48000, Box::from([[value; 2]; 16])).unwrap());
    let regions = (0..6)
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    Prepared::new(48000, pcm.into(), regions, 6)
        .unwrap()
        .with_releases(
            vec![
                Trigger::Attack,
                Trigger::Attack,
                Trigger::Attack,
                Trigger::GateRelease,
                Trigger::GateRelease,
                Trigger::GateRelease,
            ],
            ReleaseOptions::default(),
            ReleaseOptions::default(),
        )
        .unwrap()
        .with_groups(
            130,
            vec![Some(0), Some(65), Some(129), Some(0), Some(65), Some(129)],
        )
        .unwrap()
}
#[test]
fn masks_are_per_note_and_generated_children_snapshot_drafts_while_release_keeps_commit() {
    let mut rt = Runtime::new(plan(), limits()).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(1), 60, 1.).unwrap();
        let b = rt.note_on(input(2), 60, 1.).unwrap();
        rt.set_note_group(a, None, false).unwrap();
        rt.set_note_group(a, Some(65), true).unwrap();
        assert_eq!(
            rt.set_note_group(a, Some(130), true),
            Err(Error::InvalidInput)
        );
        assert!(rt.note_group_allowed(a, 65).unwrap());
        assert!(!rt.note_group_allowed(a, 129).unwrap());
        assert!(rt.note_group_allowed(b, 129).unwrap());
        assert_eq!(rt.forward_release_groups(a), Err(Error::InvalidInput));
        rt.forward_attack(a).unwrap();
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.25; 2]]);
        rt.set_note_group(a, None, false).unwrap();
        rt.set_note_group(a, Some(129), true).unwrap();
        let c = rt
            .child(a, 60, 1., false, Inheritance::Independent)
            .unwrap();
        rt.set_note_group(a, None, true).unwrap();
        assert!(!rt.note_group_allowed(c, 65).unwrap());
        assert!(rt.note_group_allowed(c, 129).unwrap());
        rt.forward_attack(c).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.75; 2]]);
        rt.key_up(a, None).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.25; 2]]); // Child 0.5 plus parent's committed release -0.25.
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_group_allowed(a, 0), Err(Error::StaleHandle));
        let n = rt.note_on(input(3), 60, 1.).unwrap();
        assert!(rt.note_group_allowed(n, 65).unwrap());
        assert!(rt.note_group_allowed(n, 129).unwrap());
    });
}
#[test]
fn filtered_attack_preflight_is_atomic_and_ungrouped_regions_are_unconditional() {
    let prepared = plan()
        .with_groups(1, vec![Some(0), None, Some(0), Some(0), None, Some(0)])
        .unwrap();
    let mut small = limits();
    small.voices = 4;
    let mut rt = Runtime::new(prepared, small).unwrap();
    support::without_heap(|| {
        let n = rt.note_on(input(1), 60, 1.).unwrap();
        assert_eq!(rt.forward_attack(n), Err(Error::Capacity)); // 3 attack + 3 reserved release.
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(rt.release_reserve().voices, 0);
        rt.set_note_group(n, None, false).unwrap();
        rt.forward_attack(n).unwrap(); // One ungrouped attack + conservative 3 release quota.
        assert_eq!(rt.voice_count(), 1);
        assert_eq!(rt.release_reserve().voices, 3);
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.25; 2]]);
        rt.key_up(n, None).unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[-0.25; 2]]);
        assert_eq!(rt.release_reserve().voices, 0);
    });
}
#[test]
fn group_layouts_follow_generations_and_retire_off_audio() {
    let old = plan();
    let new = plan().with_groups(1, vec![Some(0); 6]).unwrap();
    let (mut rt, mut transfer) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
    transfer.submit(Box::new(new)).unwrap();
    support::without_heap(|| {
        let a = rt.note_on(input(1), 60, 1.).unwrap();
        rt.set_note_group(a, None, false).unwrap();
        rt.set_note_group(a, Some(129), true).unwrap();
        rt.poll_plan_update().unwrap();
        let b = rt.note_on(input(2), 60, 1.).unwrap();
        assert_eq!(rt.note_group_allowed(b, 129), Err(Error::InvalidInput));
        assert!(rt.note_group_allowed(a, 129).unwrap());
        rt.forward_attack(a).unwrap();
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.5; 2]]);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.panic();
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    assert_eq!(transfer.retired().unwrap().prepared.group_count(), 130);
    assert!(plan().with_groups(1, vec![]).is_err());
    assert!(plan().with_groups(0, vec![Some(0); 6]).is_err());
    assert!(plan().with_groups(1, vec![Some(1); 6]).is_err());
    let mut rt = Runtime::new(plan().with_groups(0, vec![None; 6]).unwrap(), limits()).unwrap();
    support::without_heap(|| {
        let n = rt
            .note_on_with_expression(input(3), 60, 1., Expression::default())
            .unwrap();
        rt.set_note_group(n, None, false).unwrap();
        rt.forward_attack(n).unwrap();
        assert_eq!(rt.voice_count(), 3);
    });
}

#[test]
fn group_filtering_respects_take_history_and_validates_instruction_operands_before_activation() {
    use sampler_core::{Instruction, Program, Sequence, SequenceScope, Take, TakePolicy};
    let prepared = plan()
        .with_releases(
            vec![Trigger::Attack; 6],
            ReleaseOptions::default(),
            ReleaseOptions::default(),
        )
        .unwrap()
        .with_variation(
            vec![Sequence {
                takes: 2,
                scope: SequenceScope::Global,
                capacity: 1,
                policy: TakePolicy::Sequential,
            }],
            vec![
                Some(Take {
                    sequence: 0,
                    index: 0,
                }),
                Some(Take {
                    sequence: 0,
                    index: 1,
                }),
                None,
                None,
                None,
                None,
            ],
            1,
            0,
        )
        .unwrap()
        .with_groups(
            2,
            vec![Some(0), Some(0), Some(1), Some(1), Some(1), Some(1)],
        )
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    support::without_heap(|| {
        for (id, selected, expected) in [(1, false, 0.), (2, true, 0.125), (3, true, 0.25)] {
            let n = rt.note_on(input(id), 60, 1.).unwrap();
            rt.set_note_group(n, None, false).unwrap();
            rt.set_note_group(n, Some(0), selected).unwrap();
            rt.forward_attack(n).unwrap();
            let mut audio = [[0.; 2]; 1];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[expected; 2]]);
            rt.key_up(n, None).unwrap();
            rt.flush_ended(|_| true);
        }
    });
    let program = Program::new(vec![
        Instruction::End,
        Instruction::WriteGroup {
            group: Some(4),
            allowed: true,
            pending_only: false,
        },
    ])
    .unwrap();
    assert!(program.requires_note());
    let prepared = plan().with_programs(vec![program], None).unwrap();
    assert_eq!(prepared.behavior_local_count(), 5);
    assert!(matches!(
        Runtime::new(prepared, limits()),
        Err(Error::Capacity)
    ));
    let program = Program::new(vec![Instruction::WriteGroup {
        group: None,
        allowed: false,
        pending_only: false,
    }])
    .unwrap();
    let mut rt =
        Runtime::new(plan().with_programs(vec![program], None).unwrap(), limits()).unwrap();
    support::without_heap(|| {
        assert_eq!(
            rt.start_plan_behavior(rt.active_plan(), 0),
            Err(Error::InvalidInput)
        );
    });
}
