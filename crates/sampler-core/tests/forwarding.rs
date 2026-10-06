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
        assert!(rt.input_held(note).unwrap());
        assert_eq!(rt.note_off(input(5), None), Ok(note));
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

#[test]
fn event_edits_commit_once_keep_admission_and_release_identity_and_retry_atomically() {
    use sampler_core::{NotePitch, NoteProperties};
    let mut rt = Runtime::new(plan(1.), limits()).unwrap();
    support::without_heap(|| {
        let blocker = rt.trigger(input(1), 60, 1.).unwrap();
        let original = NoteProperties {
            pitch: NotePitch::Absolute(59.25),
            velocity: 0.123456789123,
        };
        let note = rt
            .note_on_pitched(
                input(2),
                original.pitch,
                original.velocity,
                Expression::default(),
            )
            .unwrap();
        let expression = rt.expression_id(note).unwrap();
        let edited = NoteProperties {
            pitch: NotePitch::Key(60),
            velocity: 0.5,
        };
        assert_eq!(rt.initial_note_properties(note), Ok(original));
        rt.edit_note_event(note, edited).unwrap();
        for invalid in [
            NoteProperties {
                pitch: NotePitch::Key(128),
                ..edited
            },
            NoteProperties {
                pitch: NotePitch::Absolute(f64::NAN),
                ..edited
            },
            NoteProperties {
                velocity: f64::NAN,
                ..edited
            },
            NoteProperties {
                velocity: -1.,
                ..edited
            },
            NoteProperties {
                velocity: 1.01,
                ..edited
            },
        ] {
            assert_eq!(rt.edit_note_event(note, invalid), Err(Error::InvalidInput));
            assert_eq!(rt.note_event(note), Ok(edited));
        }
        let reserve = rt.release_reserve();
        assert_eq!(rt.forward_attack(note), Err(Error::Capacity));
        assert_eq!(rt.note_pitch(note), Ok(original.pitch));
        assert_eq!(rt.note(note).unwrap().1, original.velocity);
        assert_eq!(rt.release_reserve(), reserve);
        rt.key_up(blocker, None).unwrap();
        rt.render(&mut [[0.; 2]; 2]).unwrap();
        rt.flush_ended(|_| true);
        assert!(rt.forward_attack(note).unwrap());
        assert_eq!(rt.note_pitch(note), Ok(edited.pitch));
        assert_eq!(rt.note(note).unwrap().1, edited.velocity);
        assert_eq!(rt.expression_id(note), Ok(expression));
        assert_eq!(rt.initial_note_properties(note), Ok(original));
        let late = NoteProperties {
            pitch: NotePitch::Key(61),
            velocity: 1.,
        };
        rt.edit_note_event(note, late).unwrap();
        assert!(!rt.forward_attack(note).unwrap());
        assert_eq!(rt.note_event(note), Ok(late));
        let mut audio = [[0.; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.5; 2]]);
        assert_eq!(rt.note_off(input(2), None), Ok(note));
        rt.render(&mut audio).unwrap();
        assert_eq!(
            audio,
            [[-0.25; 2]],
            "release uses committed key and velocity"
        );
        rt.render(&mut audio).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_event(note), Err(Error::StaleHandle));
        assert_eq!(rt.initial_note_properties(note), Err(Error::StaleHandle));
        assert_eq!(rt.edit_note_event(note, late), Err(Error::StaleHandle));
        let fresh = rt.note_on(input(3), 62, 0.75).unwrap();
        let properties = NoteProperties {
            pitch: NotePitch::Key(62),
            velocity: 0.75,
        };
        assert_eq!(rt.note_event(fresh), Ok(properties));
        assert_eq!(rt.initial_note_properties(fresh), Ok(properties));
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn event_edit_instructions_derive_register_and_note_requirements_even_when_unreachable() {
    use sampler_core::{Instruction, Program, WaitLifetime};
    for op in [
        Instruction::WriteEventKey { local: 7 },
        Instruction::WriteEventVelocity7 { local: 7 },
    ] {
        let program = Program::new(vec![Instruction::End, op])
            .unwrap()
            .with_wait_lifetime(WaitLifetime::Callback);
        assert!(program.requires_note());
        let prepared = plan(1.).with_programs(vec![program], None).unwrap();
        assert_eq!(prepared.behavior_local_count(), 8);
        let budget = Limits {
            behaviors: 1,
            behavior_cells: 7,
            behavior_fuel: 8,
            ..limits()
        };
        assert!(matches!(
            Runtime::new(prepared, budget),
            Err(Error::Capacity)
        ));
        let prepared = plan(1.)
            .with_programs(
                vec![
                    Program::new(vec![Instruction::End, op])
                        .unwrap()
                        .with_wait_lifetime(WaitLifetime::Callback),
                ],
                None,
            )
            .unwrap();
        let mut rt = Runtime::new(
            prepared,
            Limits {
                behavior_cells: 8,
                ..budget
            },
        )
        .unwrap();
        assert_eq!(
            rt.start_plan_behavior(rt.active_plan(), 0),
            Err(Error::InvalidInput)
        );
    }
}

#[test]
fn source_owned_children_keep_tails_then_return_release_quotas_without_note_off_audio() {
    use sampler_core::{
        Duration, Inheritance, Instruction, Program, Velocity, VoiceChain, VoiceProcessor,
    };
    let make_plan = || {
        plan(1.)
            .with_voice_chains(
                vec![VoiceChain::new(vec![], vec![VoiceProcessor::Gain(1.)], 3).unwrap()],
                vec![Some(0), None],
            )
            .unwrap()
            .with_programs(
                vec![
                    Program::new(vec![Instruction::Play {
                        transpose: 0,
                        velocity: Velocity::Fixed(1.),
                        inheritance: Inheritance::Independent,
                        duration: Duration::UntilSilent,
                    }])
                    .unwrap(),
                ],
                Some(0),
            )
            .unwrap()
    };
    let mut rt = Runtime::new(
        make_plan(),
        Limits {
            notes: 8,
            expressions: 8,
            behaviors: 1,
            behavior_fuel: 2,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let mut terminals = 0;
        for id in 0..8 {
            let parent = rt.trigger(input(id), 60, 1.).unwrap();
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, sampler_core::Outcome::Finished);
                true
            });
            rt.note_off(input(id), None).unwrap();
            let mut audio = [[0.; 2]; 4];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[1.; 2]; 4]);
            rt.flush_ended(|_| {
                terminals += 1;
                true
            });
            assert_eq!(rt.note_count(), 2, "the tail owns its child and parent");
            assert_eq!(rt.voice_count(), 1);
            assert_eq!(rt.release_reserve().voices, 1);
            rt.render(&mut audio[..3]).unwrap();
            assert_eq!(
                audio[..3],
                [[0.; 2]; 3],
                "source completion must not select release samples"
            );
            assert_eq!(rt.voice_count(), 0);
            // Do not flush terminals here. The next admission must reclaim the
            // completed internal child's release quota, not consume host terminals.
            assert!(!rt.key_down(parent).unwrap());
        }
        assert_eq!(
            terminals, 7,
            "only explicit flushes accept external terminals"
        );
        rt.flush_ended(|_| {
            terminals += 1;
            true
        });
        assert_eq!(terminals, 8);
        assert_eq!(
            (rt.note_count(), rt.family_count(), rt.expression_count()),
            (0, 0, 0)
        );
        assert_eq!(rt.release_reserve(), ReleaseReserve::default());
        assert_eq!(
            rt.pending_commands(),
            0,
            "whole-source lifetime needs no timer"
        );
    });
}

#[test]
fn whole_source_lifetime_handles_empty_mapping_layers_muting_and_infinite_loops() {
    use sampler_core::{
        Duration, Inheritance, Instruction, Loop, LoopMode, LoopShape, Program, Velocity,
    };
    for (mapped, looping, gain) in [
        (false, false, 1.),
        (true, false, 1.),
        (true, true, 1.),
        (true, true, 0.),
    ] {
        let region = Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain,
            envelope: Envelope::default(),
            playback: Playback {
                loop_range: looping.then_some(Loop {
                    start: 0,
                    end: 3,
                    mode: LoopMode::Continuous,
                    shape: LoopShape::Wrap,
                    passes: None,
                }),
                ..Playback::default()
            },
        };
        let prepared = Prepared::new(
            48000,
            vec![
                Pcm::new(48000, Box::from([[1.; 2]; 3])).unwrap(),
                Pcm::new(48000, Box::from([[1.; 2]; 5])).unwrap(),
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
        .with_programs(
            vec![
                Program::new(vec![Instruction::Play {
                    transpose: if mapped { 0 } else { 1 },
                    velocity: Velocity::Fixed(1.),
                    inheritance: Inheritance::Independent,
                    duration: Duration::UntilSilent,
                }])
                .unwrap(),
            ],
            Some(0),
        )
        .unwrap();
        let mut rt = Runtime::new(
            prepared,
            Limits {
                behaviors: 1,
                behavior_fuel: 2,
                ..limits()
            },
        )
        .unwrap();
        support::without_heap(|| {
            let parent = rt.trigger(input(1), 60, 1.).unwrap();
            rt.flush_behaviors(|_, _, outcome| {
                assert_eq!(outcome, sampler_core::Outcome::Finished);
                true
            });
            rt.key_up(parent, None).unwrap();
            let mut first = [[0.; 2]; 3];
            rt.render(&mut first).unwrap();
            assert_eq!(first, [[if mapped { 2. * gain } else { 0. }; 2]; 3]);
            rt.flush_ended(|_| !mapped);
            assert_eq!(rt.note_count(), if mapped { 2 } else { 0 });
            let mut rest = [[0.; 2]; 16];
            rt.render(&mut rest).unwrap();
            for (frame, actual) in rest.iter().enumerate() {
                let expected = if !mapped {
                    0.
                } else if looping {
                    2. * gain
                } else if frame < 2 {
                    gain
                } else {
                    0.
                };
                assert_eq!(*actual, [expected; 2]);
            }
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), if looping { 2 } else { 0 });
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.pending_commands()),
                (0, 0, 0)
            );
        });
    }
}
