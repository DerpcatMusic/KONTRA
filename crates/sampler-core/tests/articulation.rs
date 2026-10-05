use sampler_core::{
    Envelope, Error, Event, Expression, Input, Keyswitch, Limits, NotePitch, Pcm, Playback,
    Prepared, Protocol, Region, ReleaseOptions, ReleaseStatus, Runtime, SelectionPolicy, Sequence,
    SequenceScope, Take, TakePolicy, Trigger,
};
mod support;

fn limits() -> Limits {
    Limits {
        notes: 12,
        channels: 4,
        performances: 2,
        families: 16,
        expressions: 12,
        voices: 32,
        decisions: 32,
        commands: 16,
        behaviors: 2,
        behavior_fuel: 16,
        behavior_cells: 0,
    }
}
fn input(id: Option<i32>, channel: u8, key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 1,
        group: 2,
        channel,
        key,
        external_id: id,
    }
}
fn region(sample: usize) -> Region {
    Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }
}
fn plan(policy: SelectionPolicy) -> Prepared {
    // One unconditional and one selected mic per phase. Articulation values are stable
    // public labels; they need not be contiguous or fit a MIDI byte.
    let mut regions = Vec::new();
    let mut arts = Vec::new();
    let mut phases = Vec::new();
    for phase in [Trigger::Attack, Trigger::KeyRelease, Trigger::GateRelease] {
        for (sample, art) in [(0, None), (1, Some(0)), (2, Some(90000))] {
            regions.push(region(sample));
            arts.push(art);
            phases.push(phase);
        }
    }
    Prepared::new(
        48000,
        [0.125, 0.25, 0.5]
            .into_iter()
            .map(|v| Pcm::new(48000, Box::from([[v; 2]; 4])).unwrap())
            .collect(),
        regions,
        9,
    )
    .unwrap()
    .with_releases(phases, ReleaseOptions::default(), ReleaseOptions::default())
    .unwrap()
    .with_articulations(
        arts,
        vec![Keyswitch {
            key: 12,
            articulation: 90000,
        }],
        policy,
        policy,
    )
    .unwrap()
}

#[test]
fn release_policy_chooses_onset_or_current_without_rewriting_the_note_snapshot() {
    for policy in [SelectionPolicy::Onset, SelectionPolicy::Current] {
        let mut rt = Runtime::new(plan(policy), limits()).unwrap();
        support::without_heap(|| {
            let domain = rt.performance(0).unwrap();
            let address = input(Some(0), 0, 60);
            let channel = rt.register_channel(address.channel_address()).unwrap();
            rt.sustain(channel, true).unwrap();
            let note = rt.trigger(address, 60, 1.).unwrap();
            assert_eq!(rt.release_reserve().voices, 4); // excludes the inactive articulation
            let mut out = [[0.; 2]; 4];
            rt.render(&mut out).unwrap();
            assert_eq!(out, [[0.375; 2]; 4]);
            rt.set_articulation(domain, 90000).unwrap();
            rt.key_up(note, None).unwrap();
            rt.render(&mut out).unwrap();
            assert_eq!(
                out,
                [[if policy == SelectionPolicy::Onset {
                    0.375
                } else {
                    0.625
                }; 2]; 4]
            );
            assert_eq!(rt.note_selection(note).unwrap().articulation, 0);
            // Current state is evaluated independently at each phase's actual transition.
            rt.set_articulation(domain, 123).unwrap();
            rt.sustain(channel, false).unwrap();
            rt.render(&mut out).unwrap();
            assert_eq!(
                out,
                [[if policy == SelectionPolicy::Onset {
                    0.375
                } else {
                    0.125
                }; 2]; 4]
            );
            rt.flush_ended(|_| false);
            assert_eq!(rt.note_selection(note).unwrap().performance, domain);
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_selection(note), Err(Error::StaleHandle));
        });
    }
}

#[test]
fn performance_domains_isolate_identical_input_tuples_and_expression_channels() {
    let mut rt = Runtime::new(plan(SelectionPolicy::Onset), limits()).unwrap();
    support::without_heap(|| {
        let a = rt.performance(0).unwrap();
        let b = rt.performance(1).unwrap();
        rt.set_articulation(b, 90000).unwrap();
        let address = input(Some(1), 4, 60);
        let first = rt
            .trigger_in(a, address, NotePitch::Key(60), 1., Expression::default())
            .unwrap();
        let second = rt
            .trigger_in(b, address, NotePitch::Key(60), 1., Expression::default())
            .unwrap();
        assert_eq!(
            rt.trigger_in(b, address, NotePitch::Key(60), 1., Expression::default()),
            Err(Error::DuplicateInput)
        );
        assert_eq!(rt.note_selection(first).unwrap().articulation, 0);
        assert_eq!(rt.note_selection(second).unwrap().articulation, 90000);
        let different_member = rt
            .trigger_in(
                b,
                input(Some(2), 7, 60),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        assert_eq!(
            rt.note_selection(different_member).unwrap().articulation,
            90000
        );
        assert_eq!(rt.note_off_in(b, address, None), Ok(second));
        assert!(rt.key_down(first).unwrap());
        assert_eq!(rt.note_off(address, None), Ok(first));
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(rt.articulation(b), Ok(90000)); // panic is cleanup, not an undocumented preset reset
    });
}

#[test]
fn consumed_latched_switch_is_atomic_silent_and_not_held_by_pedals_or_bound_programs() {
    use sampler_core::{Duration, Inheritance, Instruction, Program, Velocity};
    let plan = plan(SelectionPolicy::Current)
        .with_programs(
            vec![
                Program::new(vec![Instruction::Play {
                    transpose: 0,
                    velocity: Velocity::Fixed(1.),
                    inheritance: Inheritance::Independent,
                    duration: Duration::Gate,
                }])
                .unwrap(),
            ],
            Some(0),
        )
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 2,
            expressions: 2,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let address = input(None, 0, 12);
        let channel = rt.register_channel(address.channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        rt.sostenuto(channel, true).unwrap();
        let first = rt.trigger(address, 60, 1.).unwrap(); // switch uses physical key, not transformed key
        let second = rt.trigger(address, 60, 1.).unwrap();
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(rt.family_count(), 0);
        assert_eq!(rt.pending_commands(), 0);
        assert!(rt.note_selection(first).unwrap().consumed_switch);
        assert_eq!(
            rt.release_status(first, Trigger::GateRelease),
            Ok(ReleaseStatus::Unarmed)
        );
        rt.set_articulation(domain, 7).unwrap();
        assert_eq!(rt.trigger(address, 60, 1.), Err(Error::Capacity));
        assert_eq!(rt.articulation(domain), Ok(7));
        assert_eq!(rt.note_off(address, None), Ok(first));
        assert!(!rt.note(first).unwrap().2);
        assert!(rt.key_down(second).unwrap());
        rt.flush_ended(|_| false);
        assert_eq!(rt.note_count(), 2);
        rt.flush_ended(|_| true);
        rt.all_notes_off(address.channel_address()).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
        assert_eq!(rt.expression_count(), 0);
    });
}

#[test]
fn equal_time_articulation_and_release_events_follow_host_order_and_block_boundaries() {
    for update_first in [false, true] {
        for split in [0, 1, 3, 6] {
            let mut rt = Runtime::new(plan(SelectionPolicy::Current), limits()).unwrap();
            support::without_heap(|| {
                let domain = rt.performance(0).unwrap();
                let note = rt.trigger(input(Some(0), 0, 60), 60, 1.).unwrap();
                let update = Event::Articulation(domain, 90000);
                let off = Event::KeyUp(note, None);
                rt.schedule_event(3, if update_first { update } else { off })
                    .unwrap();
                rt.schedule_event(3, if update_first { off } else { update })
                    .unwrap();
                let mut out = [[0.; 2]; 6];
                rt.render(&mut out[..split]).unwrap();
                rt.render(&mut []).unwrap();
                rt.render(&mut out[split..]).unwrap();
                assert_eq!(
                    out,
                    [
                        [0.375; 2],
                        [0.375; 2],
                        [0.375; 2],
                        [if update_first { 1.25 } else { 0.75 }; 2],
                        [if update_first { 1.25 } else { 0.75 }; 2],
                        [if update_first { 1.25 } else { 0.75 }; 2]
                    ]
                );
                rt.panic();
                rt.flush_ended(|_| true);
            });
        }
    }
}

#[test]
fn filtered_variation_and_release_reserves_match_a_linear_multimic_reference() {
    let mut regions = Vec::new();
    let mut arts = Vec::new();
    let mut phases = Vec::new();
    let mut tags = Vec::new();
    for phase in [Trigger::Attack, Trigger::GateRelease] {
        for art in [None, Some(4), Some(90000)] {
            for take in 0..3 {
                for _ in 0..2 {
                    let mut r = region(take);
                    r.velocity_low = if take == 1 { 0.5 } else { 0. };
                    regions.push(r);
                    arts.push(art);
                    phases.push(phase);
                    tags.push(Some(Take {
                        sequence: if phase == Trigger::Attack { 0 } else { 1 },
                        index: take as u32,
                    }));
                }
            }
        }
    }
    let reference = regions.clone();
    let ref_arts = arts.clone();
    let ref_phases = phases.clone();
    let plan = Prepared::new(
        48000,
        (1..=3)
            .map(|v| Pcm::new(48000, Box::from([[v as f32 / 8.; 2]])).unwrap())
            .collect(),
        regions,
        tags.len(),
    )
    .unwrap()
    .with_articulations(
        arts,
        vec![],
        SelectionPolicy::Onset,
        SelectionPolicy::Current,
    )
    .unwrap()
    .with_variation(
        vec![
            Sequence {
                takes: 3,
                policy: TakePolicy::Sequential,
                scope: SequenceScope::Global,
                capacity: 1
            };
            2
        ],
        tags,
        2,
        0,
    )
    .unwrap()
    .with_releases(phases, ReleaseOptions::default(), ReleaseOptions::default())
    .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            voices: 8,
            families: 2,
            decisions: 2,
            ..limits()
        },
    )
    .unwrap();
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        for i in 0..120 {
            let onset = [4, 90000, 88][i % 3];
            let release = [90000, 88, 4][i % 3];
            let velocity = if i % 2 == 0 { 0.25 } else { 0.75 };
            rt.set_articulation(domain, onset).unwrap();
            let note = rt
                .trigger(input(Some(i as i32), 0, 60), 60, velocity)
                .unwrap();
            assert_eq!(rt.release_reserve().voices, 4);
            for (phase, art, sequence) in [
                (Trigger::Attack, onset, 0),
                (Trigger::GateRelease, release, 1),
            ] {
                if phase == Trigger::GateRelease {
                    rt.set_articulation(domain, release).unwrap();
                    rt.key_up(note, None).unwrap();
                }
                let take = i % 3;
                assert_eq!(rt.note_take(note, phase, sequence), Ok(Some(take as u32)));
                let expected = reference
                    .iter()
                    .zip(&ref_arts)
                    .zip(&ref_phases)
                    .filter(|((r, a), p)| {
                        **p == phase
                            && a.is_none_or(|a| a == art)
                            && r.sample == take
                            && r.velocity_low <= velocity
                    })
                    .map(|((r, _), _)| (r.sample + 1) as f32 / 8. * velocity as f32)
                    .sum::<f32>();
                let mut out = [[0.; 2]; 1];
                rt.render(&mut out).unwrap();
                assert_eq!(out, [[expected; 2]]);
            }
            rt.flush_ended(|_| true);
            assert_eq!(
                (rt.note_count(), rt.voice_count(), rt.decision_count()),
                (0, 0, 0)
            );
        }
    });
}

#[test]
fn domain_and_switch_validation_fail_without_mutating_selection() {
    let mut rt = Runtime::new(plan(SelectionPolicy::Onset), limits()).unwrap();
    let other = Runtime::new(plan(SelectionPolicy::Onset), limits()).unwrap();
    let foreign = other.performance(0).unwrap();
    support::without_heap(|| {
        assert_eq!(rt.performance(2), Err(Error::InvalidInput));
        assert_eq!(rt.set_articulation(foreign, 4), Err(Error::StaleHandle));
        assert_eq!(
            rt.schedule_event(1, Event::Articulation(foreign, 4)),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            rt.trigger_in(
                foreign,
                input(None, 0, 12),
                NotePitch::Key(60),
                1.,
                Expression::default()
            ),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            rt.trigger(input(None, 0, 12), 60, f64::NAN),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.articulation(rt.performance(0).unwrap()), Ok(0));
        assert_eq!((rt.note_count(), rt.pending_commands()), (0, 0));
    });
    assert!(matches!(
        Runtime::new(
            plan(SelectionPolicy::Onset),
            Limits {
                performances: 0,
                ..limits()
            }
        ),
        Err(Error::InvalidInput)
    ));
    for switches in [
        vec![Keyswitch {
            key: 128,
            articulation: 0,
        }],
        vec![
            Keyswitch {
                key: 1,
                articulation: 0
            };
            2
        ],
    ] {
        assert!(matches!(
            plan(SelectionPolicy::Onset).with_articulations(
                vec![None; 9],
                switches,
                SelectionPolicy::Onset,
                SelectionPolicy::Onset
            ),
            Err(Error::InvalidInput)
        ));
    }
}

#[test]
fn known_onset_articulation_filters_dormant_pitch_and_scope_requirements() {
    let mut high = region(0);
    high.playback.transpose_semitones = 48.;
    for policy in [SelectionPolicy::Onset, SelectionPolicy::Current] {
        let prepared = Prepared::new(
            48000,
            vec![Pcm::new(48000, Box::from([[1.; 2]; 4])).unwrap()],
            vec![region(0), high],
            2,
        )
        .unwrap()
        .with_releases(
            vec![Trigger::GateRelease; 2],
            ReleaseOptions::default(),
            ReleaseOptions::default(),
        )
        .unwrap()
        .with_articulations(vec![Some(0), Some(1)], vec![], policy, policy)
        .unwrap();
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        support::without_heap(|| {
            let note = rt.trigger(input(Some(0), 0, 60), 60, 1.).unwrap();
            let owner = rt.expression_id(note).unwrap();
            let value = Expression {
                pitch_semitones: 12.,
                ..Expression::default()
            };
            let expected = if policy == SelectionPolicy::Onset {
                Ok(())
            } else {
                Err(Error::InvalidInput)
            };
            assert_eq!(rt.set_expression(owner, value), expected);
            assert_eq!(rt.set_expressions(&[(owner, value)]), expected);
            assert_eq!(
                rt.schedule_event(1, Event::Expression(note, value)),
                expected
            );
            rt.set_articulation(rt.performance(0).unwrap(), 1).unwrap();
            rt.key_up(note, None).unwrap();
            assert_eq!(
                rt.release_status(note, Trigger::GateRelease),
                Ok(ReleaseStatus::Selected)
            );
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
    let prepared = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]])).unwrap()],
        vec![region(0); 2],
        2,
    )
    .unwrap()
    .with_releases(
        vec![Trigger::GateRelease; 2],
        ReleaseOptions::default(),
        ReleaseOptions::default(),
    )
    .unwrap()
    .with_articulations(
        vec![Some(0), Some(1)],
        vec![],
        SelectionPolicy::Onset,
        SelectionPolicy::Onset,
    )
    .unwrap()
    .with_variation(
        vec![
            Sequence {
                takes: 1,
                policy: TakePolicy::Sequential,
                scope: SequenceScope::Channel,
                capacity: 1
            };
            2
        ],
        vec![
            Some(Take {
                sequence: 0,
                index: 0,
            }),
            Some(Take {
                sequence: 1,
                index: 0,
            }),
        ],
        2,
        0,
    )
    .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    support::without_heap(|| {
        let domain = rt.performance(0).unwrap();
        let a = rt.trigger(input(Some(0), 1, 60), 60, 1.).unwrap();
        rt.set_articulation(domain, 1).unwrap();
        let b = rt.trigger(input(Some(1), 2, 60), 60, 1.).unwrap(); // unreachable sequence 1 wasn't claimed by a
        rt.key_up(a, None).unwrap();
        rt.key_up(b, None).unwrap();
        assert_eq!(rt.note_take(a, Trigger::GateRelease, 0), Ok(Some(0)));
        assert_eq!(rt.note_take(a, Trigger::GateRelease, 1), Ok(None));
        assert_eq!(rt.note_take(b, Trigger::GateRelease, 1), Ok(Some(0)));
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn delayed_children_keep_original_mapping_and_domain_but_capture_their_own_onset_state() {
    use sampler_core::{Duration, Inheritance, Instruction, Program, Velocity};
    let old = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[0.5; 2]])).unwrap()],
        vec![region(0)],
        1,
    )
    .unwrap()
    .with_releases(
        vec![Trigger::GateRelease],
        ReleaseOptions::default(),
        ReleaseOptions::default(),
    )
    .unwrap()
    .with_articulations(
        vec![Some(90000)],
        vec![],
        SelectionPolicy::Onset,
        SelectionPolicy::Onset,
    )
    .unwrap()
    .with_controllers(
        vec![vec![sampler_core::ControllerCondition {
            controller: 1,
            low: 90000,
            high: 90000,
        }]],
        1,
    )
    .unwrap()
    .with_programs(
        vec![
            Program::new(vec![
                Instruction::Wait(2),
                Instruction::Play {
                    transpose: 0,
                    velocity: Velocity::Fixed(1.),
                    inheritance: Inheritance::Independent,
                    duration: Duration::Frames(1),
                },
                Instruction::End,
            ])
            .unwrap(),
        ],
        None,
    )
    .unwrap();
    let new = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[2.; 2]])).unwrap()],
        vec![region(0)],
        1,
    )
    .unwrap()
    .with_releases(
        vec![Trigger::GateRelease],
        ReleaseOptions::default(),
        ReleaseOptions::default(),
    )
    .unwrap()
    .with_articulations(
        vec![Some(90000)],
        vec![],
        SelectionPolicy::Onset,
        SelectionPolicy::Onset,
    )
    .unwrap();
    let new = new
        .with_controllers(
            vec![vec![sampler_core::ControllerCondition {
                controller: 2,
                low: 123,
                high: 123,
            }]],
            1,
        )
        .unwrap();
    let (mut rt, mut control) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
    control.submit(Box::new(new)).unwrap();
    support::without_heap(|| {
        let domain = rt.performance(1).unwrap();
        let root = rt
            .trigger_in(
                domain,
                input(Some(0), 3, 60),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        let old_plan = rt.note_plan(root).unwrap();
        rt.start_behavior(root, 0).unwrap();
        rt.poll_plan_update().unwrap();
        rt.set_articulation(domain, 90000).unwrap();
        rt.set_controller(domain, 1, 90000).unwrap();
        rt.set_controller(domain, 2, 123).unwrap();
        let mut out = [[0.; 2]; 5];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[0.; 2], [0.; 2], [0.; 2], [0.5; 2], [0.; 2]]);
        let new_note = rt
            .trigger_in(
                domain,
                input(Some(1), 5, 60),
                NotePitch::Key(60),
                1.,
                Expression::default(),
            )
            .unwrap();
        rt.key_up(root, None).unwrap(); // root's onset 0 is unmapped even though the domain changed
        rt.key_up(new_note, None).unwrap();
        rt.render(&mut out[..1]).unwrap();
        assert_eq!(out[0], [2.; 2]);
        assert_eq!(rt.note_plan(root), Ok(old_plan));
        assert_eq!(rt.note_selection(root).unwrap().articulation, 0);
        assert_eq!(rt.note_controller(root, 1).unwrap(), 0);
        rt.flush_behaviors(|_, _, _| true);
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(control.retired().unwrap());
}

#[test]
fn old_switch_note_off_cannot_release_a_new_musical_note_after_mapping_replacement() {
    let old = plan(SelectionPolicy::Onset);
    let new = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[0.5; 2]; 4])).unwrap()],
        vec![region(0)],
        1,
    )
    .unwrap();
    let (mut rt, mut control) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
    control.submit(Box::new(new)).unwrap();
    support::without_heap(|| {
        let input = input(None, 0, 12);
        let switch = rt.trigger(input, 60, 1.).unwrap();
        rt.poll_plan_update().unwrap();
        let musical = rt.trigger(input, 60, 1.).unwrap();
        assert!(!rt.note_selection(musical).unwrap().consumed_switch);
        assert_eq!(rt.note_off(input, None), Ok(switch));
        assert!(rt.key_down(musical).unwrap());
        let mut out = [[0.; 2]; 1];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[0.5; 2]]);
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(rt.note_off(input, None), Ok(musical));
        rt.flush_ended(|_| true);
        let channel = rt.register_channel(input.channel_address()).unwrap();
        rt.sustain(channel, true).unwrap();
        let reuse = rt.trigger(input, 60, 1.).unwrap();
        assert!(!rt.note_selection(reuse).unwrap().consumed_switch);
        rt.key_up(reuse, None).unwrap();
        assert!(rt.note(reuse).unwrap().2); // reused switch slot cannot bypass pedals
        rt.panic();
        rt.flush_ended(|_| true);
    });
    drop(control.retired().unwrap());
}
