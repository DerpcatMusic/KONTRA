//! Synthetic source-template routing oracles; no native extant-voice parity claim.
use sampler_core::*;
mod support;
const ZONE: u32 = 9001;
const SAMPLER: u32 = 42;
const WT: u32 = 11;
const SELECTORS: [i32; 3] = [1234, 1235, 1236];
fn input(id: i32, key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(id),
    }
}
fn switch(group: u32, low: u8, high: u8) -> Program {
    let mut code = vec![Instruction::SetLocal {
        local: 0,
        value: i64::from(ZONE),
    }];
    for (index, value) in [i64::from(group), i64::from(low), i64::from(high)]
        .into_iter()
        .enumerate()
    {
        code.extend([
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[index]),
            },
            Instruction::SetLocal { local: 2, value },
            Instruction::Op(Op::SetZoneParameter {
                zone: 0,
                parameter: 1,
                value: 2,
                result: 3 + index as u16,
                selectors: SELECTORS,
                present: 7,
            }),
        ]);
    }
    code.extend([
        Instruction::SetLocal {
            local: 1,
            value: i64::from(SELECTORS[0]),
        },
        Instruction::Op(Op::ZoneParameter {
            zone: 0,
            parameter: 1,
            selectors: SELECTORS,
            present: 7,
            local: 6,
        }),
        Instruction::Wait(1),
        Instruction::Op(Op::ZoneParameter {
            zone: 0,
            parameter: 1,
            selectors: SELECTORS,
            present: 7,
            local: 7,
        }),
        Instruction::End,
    ]);
    Program::new(code)
        .unwrap()
        .with_script_instance(ScriptInstanceId(0))
        .with_wait_lifetime(WaitLifetime::Callback)
}
fn template(wave: bool) -> Region {
    Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: Some(60),
        velocity_low: 0.,
        velocity_high: 1.,
        gain: if wave { 0.75 } else { 0.5 },
        envelope: Envelope::new(
            if wave { 7 } else { 0 },
            0,
            0,
            if wave { 0.75 } else { 1. },
            0,
        )
        .unwrap(),
        playback: if wave {
            Playback {
                wavetable: Some(Wavetable::default()),
                ..Default::default()
            }
        } else {
            Playback {
                loop_range: Some(Loop {
                    start: 0,
                    end: 4096,
                    mode: LoopMode::Continuous,
                    shape: LoopShape::Wrap,
                    passes: None,
                }),
                ..Default::default()
            }
        },
    }
}
fn make(routing: bool, only: bool) -> Prepared {
    let pcm = Pcm::new(
        48_000,
        (0..4096)
            .map(|i| {
                let x = (((i * 73) % 2048) as f32 - 1024.) / 8192.;
                [x, -x * 0.5]
            })
            .collect(),
    )
    .unwrap();
    let choices: Vec<bool> = if routing {
        vec![false, true]
    } else {
        vec![only]
    };
    let len = choices.len();
    let mut plan = Prepared::new(
        48_000,
        vec![pcm],
        choices.iter().map(|&w| template(w)).collect(),
        len,
    )
    .unwrap()
    .with_groups(
        2,
        choices
            .iter()
            .map(|&w| Some(if w { 0 } else { 1 }))
            .collect(),
    )
    .unwrap()
    .with_source_groups(vec![WT, SAMPLER])
    .unwrap()
    .with_source_zones(vec![ZONE; len])
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant; len])
    .unwrap()
    .with_voice_chains(
        vec![
            VoiceChain::new(vec![Processor::Gain(0.5)], vec![], 0).unwrap(),
            VoiceChain::new(vec![Processor::Gain(0.25)], vec![], 0).unwrap(),
        ],
        choices.iter().map(|&w| Some(usize::from(w))).collect(),
    )
    .unwrap()
    .with_buses(
        vec![
            Bus {
                processors: vec![Processor::Gain(2.)],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 0,
            },
            Bus {
                processors: vec![Processor::Gain(3.)],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 0,
            },
        ],
        choices.iter().map(|&w| Some(usize::from(!w))).collect(),
    )
    .unwrap()
    .with_voice_modulation(
        vec![
            ModProgram {
                sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, ModTarget::Pan, -0.2)],
                ..Default::default()
            },
            ModProgram {
                sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, ModTarget::Pan, 0.3)],
                ..Default::default()
            },
        ],
        choices.iter().map(|&w| Some(usize::from(w))).collect(),
        vec![0; len],
    )
    .unwrap()
    .with_script_instances(vec![vec![]])
    .unwrap()
    .with_programs(vec![switch(WT, 0, 127), switch(SAMPLER, 60, 60)], None)
    .unwrap();
    if routing {
        plan = plan
            .with_source_zone_identities(
                vec![SourceZoneIdentity {
                    id: Some(ZONE),
                    home: SourceZoneHome {
                        group: SAMPLER,
                        low_key: 60,
                        high_key: 60,
                    },
                    region: Some(0),
                }],
                1,
                4096,
            )
            .unwrap();
        plan = plan
            .with_zone_routing(
                3,
                vec![ZoneRouting {
                    source_zone: ZONE,
                    group: SAMPLER,
                    low_key: 60,
                    high_key: 60,
                    variants: vec![
                        ZoneRouteVariant {
                            group: SAMPLER,
                            region: Some(0),
                        },
                        ZoneRouteVariant {
                            group: WT,
                            region: Some(1),
                        },
                    ],
                }],
                ZoneRoutingLimits {
                    zones: 1,
                    variants: 2,
                    candidates: 256,
                    jobs: 16,
                    bytes: 64 * 1024,
                },
                &Tuning::default(),
            )
            .unwrap();
    }
    plan
}
fn runtime(p: Prepared) -> Runtime {
    let mut limits = Limits::for_plan(&p, 16, 16);
    limits.behaviors = 16;
    limits.behavior_fuel = 4096;
    limits.behavior_cells = p.behavior_local_count() * limits.behaviors;
    Runtime::new(p, limits).unwrap()
}
fn bits(x: &[Frame]) -> Vec<[u32; 2]> {
    x.iter().map(|f| f.map(f32::to_bits)).collect()
}
#[test]
fn zone_source_a_b_a_matches_static_templates_and_queries_without_heap() {
    for block in [1, 7, 64, 129] {
        let mut rt = runtime(make(true, false));
        for (round, wave) in [false, true, false].into_iter().enumerate() {
            if round != 0 {
                let group = if wave { WT } else { SAMPLER };
                let before = if wave { SAMPLER } else { WT };
                let mut callback = None;
                support::without_heap(|| {
                    let id = rt
                        .start_plan_behavior(rt.active_plan(), if wave { 0 } else { 1 })
                        .unwrap();
                    assert_eq!(rt.behavior_local(id, 6), Ok(i64::from(before)));
                    let ids = [3, 4, 5].map(|r| rt.behavior_local(id, r).unwrap());
                    assert!(ids[0] > 0 && ids[0] < ids[1] && ids[1] < ids[2]);
                    assert_eq!(
                        rt.zone_parameter_in(rt.active_plan(), ZONE, ZoneParameter::Group),
                        Ok(group as i32)
                    );
                    rt.render(&mut [[0.; 2]; 1]).unwrap();
                    // The wait is due at the block END. Service that exact
                    // boundary before reading the callback's post-wait local.
                    rt.render(&mut []).unwrap();
                    callback = Some(id);
                });
                assert_eq!(
                    rt.behavior_local(callback.unwrap(), 7),
                    Ok(i64::from(group))
                );
            }
            let mut reference = runtime(make(false, wave));
            let mut actual = [[0.; 2]; 256];
            let mut expected = actual;
            support::without_heap(|| {
                let n = rt.trigger(input(round as i32 + 1, 60), 60, 1.).unwrap();
                reference.trigger(input(1, 60), 60, 1.).unwrap();
                for chunk in actual.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                for chunk in expected.chunks_mut(block) {
                    reference.render(chunk).unwrap();
                }
                rt.key_up(n, None).unwrap();
                rt.render(&mut [[0.; 2]; 1]).unwrap();
                rt.flush_ended(|_| true);
                assert_eq!(rt.voice_count(), 0);
            });
            assert_eq!(
                bits(&actual),
                bits(&expected),
                "full source/envelope/chain/bus/modulation wave={wave} block={block}"
            );
        }
    }
}
#[test]
fn held_source_keeps_original_template_after_committed_home_change() {
    let mut rt = runtime(make(true, false));
    let mut reference = runtime(make(false, false));
    let mut actual = [[0.; 2]; 128];
    let mut expected = actual;
    support::without_heap(|| {
        rt.trigger(input(1, 60), 60, 1.).unwrap();
        reference.trigger(input(1, 60), 60, 1.).unwrap();
        rt.render(&mut actual[..64]).unwrap();
        reference.render(&mut expected[..64]).unwrap();
        rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        rt.render(&mut actual[64..]).unwrap();
        reference.render(&mut expected[64..]).unwrap();
        assert_eq!(rt.voice_count(), 1);
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), ZONE, ZoneParameter::Group),
            Ok(WT as i32)
        );
    });
    assert_eq!(bits(&actual), bits(&expected));
    let foreign = reference.active_plan();
    assert_eq!(
        rt.zone_parameter_in(foreign, ZONE, ZoneParameter::Group),
        Err(Error::StaleHandle)
    );
}

// Exact callback-ID reservation/near-exhaustion checks also live in the private
// module's synthetic tests; these public oracles exercise real waiter delivery.
#[test]
fn zone_reserved_completions_are_exact_instance_and_wait_async_unblocks_at_saturated_arena() {
    let callback = |instance, code| {
        Program::new(code)
            .unwrap()
            .with_script_instance(ScriptInstanceId(instance))
            .with_wait_lifetime(WaitLifetime::Callback)
    };
    let mut code = vec![Instruction::SetLocal {
        local: 0,
        value: i64::from(ZONE),
    }];
    for (index, value) in [i64::from(WT), 0, 127].into_iter().enumerate() {
        code.extend([
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[index]),
            },
            Instruction::SetLocal { local: 2, value },
            Instruction::Op(Op::SetZoneParameter {
                zone: 0,
                parameter: 1,
                value: 2,
                result: 3 + index as u16,
                selectors: SELECTORS,
                present: 7,
            }),
        ]);
    }
    code.extend([
        Instruction::Op(Op::WaitMidi { local: 5 }),
        Instruction::Op(Op::Emit {
            service: 702,
            args: 5,
            count: 1,
            text: None,
        }),
        Instruction::End,
    ]);
    let completion = |instance, service| {
        callback(
            instance,
            vec![
                Instruction::Op(Op::ReadHost { local: 0, slot: 6 }),
                Instruction::Op(Op::ReadHost { local: 1, slot: 7 }),
                Instruction::Op(Op::Emit {
                    service,
                    args: 0,
                    count: 2,
                    text: None,
                }),
                Instruction::End,
            ],
        )
    };
    let plan = make(true, false)
        .with_script_instances(vec![vec![], vec![]])
        .unwrap()
        .with_programs(
            vec![callback(0, code), completion(1, 701), completion(0, 700)],
            None,
        )
        .unwrap()
        .with_signal_programs(vec![
            SignalProgram {
                signal: MIDI_ASYNC_SIGNAL,
                program: 1,
                stage: 0,
            },
            SignalProgram {
                signal: MIDI_ASYNC_SIGNAL,
                program: 2,
                stage: 0,
            },
        ])
        .unwrap();
    let mut limits = Limits::for_plan(&plan, 4, 4);
    limits.behaviors = 4;
    limits.behavior_fuel = 4096;
    limits.behavior_cells = plan.behavior_local_count() * 4;
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut events = Vec::with_capacity(4);
    support::without_heap(|| {
        rt.begin_behavior_block(4096, None);
        rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        for _ in 0..8 {
            let at = rt.now();
            rt.begin_behavior_block(4096, None);
            rt.render(&mut [[0.; 2]; 8]).unwrap();
            assert_eq!(rt.now(), at + 8);
            rt.drain_effects(|e| {
                assert!(events.len() < 4);
                events.push((e.service, e.args[0], e.args[1]));
                true
            });
        }
    });
    assert_eq!(
        events,
        vec![(700, 1, 1), (700, 2, 1), (700, 3, 1), (702, 3, 0)],
        "same original instance, exactly one completion per ID, then wait_async resumes"
    );
    assert_eq!(
        rt.zone_parameter_in(rt.active_plan(), ZONE, ZoneParameter::Group),
        Ok(WT as i32)
    );
    assert_eq!(
        rt.zone_routing_report(rt.active_plan())
            .unwrap()
            .unwrap()
            .pending_jobs,
        0
    );
    support::without_heap(|| {
        rt.flush_behaviors(|_, _, _| true);
        // All three reserved slots and the owner slot must be returned.
        // Repetition proves no hidden permanent callback reservation.
        rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        rt.panic();
        rt.flush_behaviors(|_, _, _| true);
    });
    assert_eq!(
        rt.zone_routing_report(rt.active_plan())
            .unwrap()
            .unwrap()
            .pending_jobs,
        0
    );
}

fn route_single(p: Prepared) -> Result<Prepared, ZoneRoutingBuildError> {
    p.with_source_zone_identities(
        vec![SourceZoneIdentity {
            id: Some(0),
            home: SourceZoneHome {
                group: SAMPLER,
                low_key: 60,
                high_key: 60,
            },
            region: Some(0),
        }],
        1,
        4096,
    )
    .unwrap()
    .with_zone_routing(
        3,
        vec![ZoneRouting {
            source_zone: 0,
            group: SAMPLER,
            low_key: 60,
            high_key: 60,
            variants: vec![ZoneRouteVariant {
                group: SAMPLER,
                region: Some(0),
            }],
        }],
        ZoneRoutingLimits {
            zones: 1,
            variants: 1,
            candidates: 256,
            jobs: 4,
            bytes: 64 * 1024,
        },
        &Tuning::default(),
    )
}

#[test]
fn absolute_pitch_and_dynamic_sample_updates_are_explicit_before_mutation() {
    let mut routed = runtime(make(true, false));
    let plan = routed.active_plan();
    support::without_heap(|| {
        assert_eq!(
            routed.trigger_pitched(
                input(1, 60),
                NotePitch::Absolute(60.),
                1.,
                Expression::default()
            ),
            Err(Error::UnsupportedZoneRoutingPitch)
        );
        assert_eq!(
            routed.note_on_pitched(
                input(1, 60),
                NotePitch::Absolute(60.),
                1.,
                Expression::default()
            ),
            Err(Error::UnsupportedZoneRoutingPitch)
        );
        assert_eq!(routed.voice_count(), 0);
        assert_eq!(
            routed.zone_parameter_in(plan, ZONE, ZoneParameter::Group),
            Ok(SAMPLER as i32)
        );
    });
    let mut ordinary = runtime(make(false, false));
    support::without_heap(|| {
        ordinary
            .trigger_pitched(
                input(1, 60),
                NotePitch::Absolute(60.),
                1.,
                Expression::default(),
            )
            .unwrap();
    });
    let targets = || {
        vec![SampleTarget {
            id: SampleTargetId(100),
            regions: vec![0].into(),
        }]
    };
    let limits = SampleRevisionLimits {
        slots: 2,
        resident_bytes: 1 << 20,
    };
    assert!(matches!(
        route_single(
            make(false, false)
                .with_sample_targets(targets(), limits)
                .unwrap()
        ),
        Err(ZoneRoutingBuildError::Unsupported(
            ZoneRoutingFeature::DynamicSampleUpdates
        ))
    ));
    assert!(matches!(
        route_single(make(false, false))
            .unwrap()
            .with_sample_targets(targets(), limits),
        Err(Error::UnsupportedZoneRoutingSampleUpdates)
    ));
    let release = ReleaseOptions {
        duration: Some(4),
        ..Default::default()
    };
    assert!(matches!(
        route_single(
            make(false, false)
                .with_releases(vec![Trigger::KeyRelease], release, Default::default())
                .unwrap()
        ),
        Err(ZoneRoutingBuildError::Unsupported(
            ZoneRoutingFeature::NonAttackTemplate
        ))
    ));
}

#[test]
fn physical_roster_ops_keep_sparse_zero_filtered_and_unknown_ids() {
    let rows = vec![
        SourceZoneIdentity {
            id: Some(0),
            home: SourceZoneHome {
                group: SAMPLER,
                low_key: 60,
                high_key: 60,
            },
            region: Some(0),
        },
        SourceZoneIdentity {
            id: Some(7),
            home: SourceZoneHome {
                group: WT,
                low_key: 12,
                high_key: 84,
            },
            region: None,
        },
        SourceZoneIdentity {
            id: Some(2018),
            home: SourceZoneHome {
                group: 92,
                low_key: 0,
                high_key: 127,
            },
            region: None,
        },
        SourceZoneIdentity {
            id: None,
            home: SourceZoneHome {
                group: 4,
                low_key: 1,
                high_key: 9,
            },
            region: None,
        },
    ];
    let mut code = vec![Instruction::Op(Op::SourceZoneCount { local: 5 })];
    for index in 0..5 {
        code.push(Instruction::SetLocal {
            local: 0,
            value: index,
        });
        code.push(Instruction::Op(Op::SourceZoneIdentity {
            index: 0,
            local: 6 + index as u16,
        }));
    }
    code.extend([
        Instruction::SetLocal { local: 0, value: 7 },
        Instruction::SetLocal {
            local: 1,
            value: i64::from(SELECTORS[0]),
        },
        Instruction::Op(Op::ZoneParameter {
            zone: 0,
            parameter: 1,
            selectors: SELECTORS,
            present: 7,
            local: 11,
        }),
        Instruction::End,
    ]);
    let p = make(false, false)
        .with_source_zone_identities(rows, 4, 4096)
        .unwrap()
        .with_programs(
            vec![
                Program::new(code)
                    .unwrap()
                    .with_script_instance(ScriptInstanceId(0))
                    .with_wait_lifetime(WaitLifetime::Callback),
            ],
            None,
        )
        .unwrap();
    assert_eq!(p.source_zone_identity_region(0), Some(0));
    assert_eq!(p.source_zone_identity_region(7), None);
    let mut rt = runtime(p);
    support::without_heap(|| {
        let id = rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        assert_eq!(rt.behavior_local(id, 5), Ok(4));
        assert_eq!(
            [6, 7, 8, 9, 10].map(|r| rt.behavior_local(id, r).unwrap()),
            [0, 7, 2018, -1, -1]
        );
        assert_eq!(rt.behavior_local(id, 11), Ok(i64::from(WT)));
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), 7, ZoneParameter::LowKey),
            Ok(12)
        );
    });
    assert!(
        std::mem::size_of::<Instruction>() <= 32,
        "shared instruction layout budget"
    );
}

#[test]
fn eof_end_and_first_real_wait_commit_but_fuel_preemption_does_not() {
    for end in [false, true] {
        let mut code = vec![
            Instruction::SetLocal {
                local: 0,
                value: i64::from(ZONE),
            },
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[0]),
            },
            Instruction::SetLocal {
                local: 2,
                value: i64::from(WT),
            },
            Instruction::Op(Op::SetZoneParameter {
                zone: 0,
                parameter: 1,
                value: 2,
                result: 3,
                selectors: SELECTORS,
                present: 7,
            }),
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[1]),
            },
            Instruction::SetLocal { local: 2, value: 0 },
            Instruction::Op(Op::SetZoneParameter {
                zone: 0,
                parameter: 1,
                value: 2,
                result: 4,
                selectors: SELECTORS,
                present: 7,
            }),
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[2]),
            },
            Instruction::SetLocal {
                local: 2,
                value: 127,
            },
            Instruction::Op(Op::SetZoneParameter {
                zone: 0,
                parameter: 1,
                value: 2,
                result: 5,
                selectors: SELECTORS,
                present: 7,
            }),
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[0]),
            },
            Instruction::Op(Op::ZoneParameter {
                zone: 0,
                parameter: 1,
                selectors: SELECTORS,
                present: 7,
                local: 6,
            }),
            Instruction::Wait(0),
            Instruction::Op(Op::ZoneParameter {
                zone: 0,
                parameter: 1,
                selectors: SELECTORS,
                present: 7,
                local: 7,
            }),
        ];
        if end {
            code.push(Instruction::End);
        }
        let p = make(true, false)
            .with_programs(
                vec![
                    Program::new(code)
                        .unwrap()
                        .with_script_instance(ScriptInstanceId(0))
                        .with_wait_lifetime(WaitLifetime::Callback),
                ],
                None,
            )
            .unwrap();
        let mut rt = runtime(p);
        support::without_heap(|| {
            rt.begin_behavior_block(10, None);
            let id = rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
            assert_eq!(
                rt.zone_parameter_in(rt.active_plan(), ZONE, ZoneParameter::Group),
                Ok(SAMPLER as i32)
            );
            assert_eq!(
                rt.zone_routing_report(rt.active_plan())
                    .unwrap()
                    .unwrap()
                    .pending_jobs,
                3
            );
            rt.begin_behavior_block(100, None);
            rt.render(&mut [[0.; 2]; 1]).unwrap();
            assert_eq!(rt.behavior_local(id, 6), Ok(i64::from(SAMPLER)));
            assert_eq!(
                rt.behavior_local(id, 7),
                Ok(i64::from(SAMPLER)),
                "wait(0) never suspended"
            );
            assert_eq!(
                rt.zone_parameter_in(rt.active_plan(), ZONE, ZoneParameter::Group),
                Ok(WT as i32)
            );
            assert_eq!(
                rt.zone_routing_report(rt.active_plan())
                    .unwrap()
                    .unwrap()
                    .pending_jobs,
                0
            );
        });
    }
}

fn bounded_plan(keys: (u8, u8), routing: bool, fade: bool) -> Prepared {
    let pcm = Pcm::new(48_000, vec![[0.25; 2]; 256].into_boxed_slice()).unwrap();
    let region = || Region {
        sample: 0,
        key_low: keys.0,
        key_high: keys.1,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::new(0, 0, 0, 1., 0).unwrap(),
        playback: Playback::default(),
    };
    let n = if routing { 2 } else { 1 };
    let mut p = Prepared::new(
        48_000,
        vec![pcm],
        (0..n).map(|_| region()).collect(),
        n * 128,
    )
    .unwrap()
    .with_groups(
        2,
        if routing {
            vec![Some(1), Some(0)]
        } else {
            vec![Some(1)]
        },
    )
    .unwrap()
    .with_source_groups(vec![WT, SAMPLER])
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant; n])
    .unwrap()
    .with_zone_fades(vec![
        if fade {
            ZoneFades {
                key_in: 2,
                key_out: 2,
                ..Default::default()
            }
        } else {
            ZoneFades::default()
        };
        n
    ])
    .unwrap()
    .with_script_instances(vec![vec![]])
    .unwrap()
    .with_programs(
        vec![
            switch(WT, 0, 127),
            switch(SAMPLER, 0, 127),
            switch(SAMPLER, 49, 51),
        ],
        None,
    )
    .unwrap();
    if routing {
        p = p
            .with_source_zone_identities(
                vec![SourceZoneIdentity {
                    id: Some(ZONE),
                    home: SourceZoneHome {
                        group: SAMPLER,
                        low_key: 0,
                        high_key: 127,
                    },
                    region: Some(0),
                }],
                1,
                4096,
            )
            .unwrap()
            .with_zone_routing(
                3,
                vec![ZoneRouting {
                    source_zone: ZONE,
                    group: SAMPLER,
                    low_key: 0,
                    high_key: 127,
                    variants: vec![
                        ZoneRouteVariant {
                            group: SAMPLER,
                            region: Some(0),
                        },
                        ZoneRouteVariant {
                            group: WT,
                            region: Some(1),
                        },
                    ],
                }],
                ZoneRoutingLimits {
                    zones: 1,
                    variants: 2,
                    candidates: 256,
                    jobs: 16,
                    bytes: 64 * 1024,
                },
                &Tuning::default(),
            )
            .unwrap();
    }
    p
}

#[test]
fn authored_initial_home_restores_exact_admitted_mask_and_edited_key_fade_endpoints() {
    let mut rt = runtime(bounded_plan((50, 70), true, false));
    assert_eq!(
        rt.zone_routing_report(rt.active_plan())
            .unwrap()
            .unwrap()
            .partial_initial_keys,
        107
    );
    for (round, voices) in [0, 1, 0].into_iter().enumerate() {
        support::without_heap(|| {
            if round != 0 {
                rt.start_plan_behavior(rt.active_plan(), if round == 1 { 0 } else { 1 })
                    .unwrap();
            }
            rt.trigger(input(round as i32 + 1, 49), 49, 1.).unwrap();
            assert_eq!(
                rt.voice_count(),
                voices,
                "initial/restored source uses admitted50..70, destination uses requested0..127"
            );
            rt.panic();
            rt.flush_behaviors(|_, _, _| true);
            rt.flush_ended(|_| true);
        });
    }
    for key in [48, 49, 50, 51, 52] {
        let mut routed = runtime(bounded_plan((50, 70), true, true));
        let mut reference = runtime(bounded_plan((49, 51), false, true));
        let mut actual = [[0.; 2]; 8];
        let mut expected = actual;
        support::without_heap(|| {
            routed.start_plan_behavior(routed.active_plan(), 2).unwrap();
            routed.trigger(input(1, key), key, 1.).unwrap();
            reference.trigger(input(1, key), key, 1.).unwrap();
            routed.render(&mut actual).unwrap();
            reference.render(&mut expected).unwrap();
        });
        assert_eq!(
            bits(&actual),
            bits(&expected),
            "routed key/fade endpoints at{key}"
        );
    }
}

#[test]
fn lower_binds_optional_bank_to_physical_roster_after_complete_normal_recipe_pipeline() {
    use sampler_ir as ir;
    let asset = |name: &str| ir::Asset {
        location: ir::AssetLocation::Path(name.into()),
        residency: Default::default(),
        encoding: ir::Encoding::Wav,
        root_key: Some(60),
        loops: vec![],
    };
    let zone = |sample, group| ir::Zone {
        group: Some(ir::GroupRef(group)),
        keys: ir::KeyRange { low: 60, high: 60 },
        ..ir::Zone::new(ir::AssetRef(sample))
    };
    let mut instrument = ir::Instrument {
        assets: vec![asset("sampler.wav"), asset("cycle.wav")],
        groups: vec![
            ir::Group::default(),
            ir::Group {
                wavetable: Some(Wavetable::default()),
                ..Default::default()
            },
        ],
        zones: vec![zone(0, 0), zone(1, 1)],
        zone_routing_mode: Some(3),
        ..Default::default()
    };
    instrument.source_indices.groups = vec![None; 43];
    instrument.source_indices.groups[42] = Some(ir::GroupRef(0));
    instrument.source_indices.groups[11] = Some(ir::GroupRef(1));
    instrument.source_indices.zones = vec![Some(ir::ZoneRef(0)), None];
    instrument.source_indices.zone_catalog = vec![
        ir::SourceZone {
            id: Some(0),
            home: ir::SourceZoneHome {
                group: SAMPLER,
                low_key: 60,
                high_key: 60,
            },
        },
        ir::SourceZone {
            id: Some(7),
            home: ir::SourceZoneHome {
                group: WT,
                low_key: 0,
                high_key: 127,
            },
        },
    ];
    instrument.zone_routing = vec![
        ir::ZoneRouting {
            source_zone: 0,
            group: SAMPLER,
            low_key: 60,
            high_key: 60,
            variants: vec![
                ir::ZoneRouteVariant {
                    group: SAMPLER,
                    zone: Some(ir::ZoneRef(0)),
                },
                ir::ZoneRouteVariant {
                    group: WT,
                    zone: Some(ir::ZoneRef(1)),
                },
            ],
        },
        ir::ZoneRouting {
            source_zone: 7,
            group: WT,
            low_key: 0,
            high_key: 127,
            variants: vec![ir::ZoneRouteVariant {
                group: WT,
                zone: None,
            }],
        },
    ];
    let pcm = || {
        Pcm::new(
            48_000,
            (0..4096)
                .map(|i| {
                    let x = (i as f32 * std::f32::consts::TAU / 2048.).sin() * 0.1;
                    [x, x]
                })
                .collect(),
        )
        .unwrap()
    };
    let plan = sampler_core::lower::lower_with(
        &instrument,
        48_000,
        vec![pcm(), pcm()],
        &sampler_core::lower::Options { mpe: None },
        |_, p| Ok(p),
    )
    .unwrap();
    assert_eq!(plan.source_zone_identity_count(), 2);
    assert_eq!(plan.source_zone_identity(0), Some(0));
    assert_eq!(plan.source_zone_identity_region(0), Some(0));
    assert_eq!(plan.source_zone_identity_region(7), None);
    // Build the same operation sequence for explicit native ID0.
    let code = Program::new(vec![
        Instruction::SetLocal { local: 0, value: 0 },
        Instruction::SetLocal {
            local: 1,
            value: i64::from(SELECTORS[0]),
        },
        Instruction::SetLocal {
            local: 2,
            value: i64::from(WT),
        },
        Instruction::Op(Op::SetZoneParameter {
            zone: 0,
            parameter: 1,
            value: 2,
            result: 3,
            selectors: SELECTORS,
            present: 7,
        }),
        Instruction::End,
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
    .with_wait_lifetime(WaitLifetime::Callback);
    let plan = plan
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_programs(vec![code], None)
        .unwrap();
    let mut rt = runtime(plan);
    let report = rt.zone_routing_report(rt.active_plan()).unwrap().unwrap();
    assert_eq!(
        (
            report.zones,
            report.templates,
            report.catalog_only_initial_homes,
            report.unavailable_destinations
        ),
        (2, 3, 1, 1)
    );
    support::without_heap(|| {
        let id = rt.start_plan_behavior(rt.active_plan(), 0).unwrap();
        assert_eq!(rt.behavior_local(id, 3), Ok(1));
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), 0, ZoneParameter::Group),
            Ok(WT as i32)
        );
        assert_eq!(
            rt.zone_parameter_in(rt.active_plan(), 7, ZoneParameter::Group),
            Ok(WT as i32)
        );
        rt.trigger(input(1, 60), 60, 1.).unwrap();
        assert_eq!(rt.voice_count(), 1);
        rt.render(&mut [[0.; 2]; 8]).unwrap();
    });
}

#[test]
fn unrelated_ordinary_release_templates_remain_supported_with_attack_routing() {
    let pcm = Pcm::new(48_000, vec![[0.1; 2]; 256].into_boxed_slice()).unwrap();
    let region = || Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::new(0, 0, 0, 1., 0).unwrap(),
        playback: Playback::default(),
    };
    let release = ReleaseOptions {
        duration: Some(4),
        ..Default::default()
    };
    let p = Prepared::new(48_000, vec![pcm], vec![region(), region()], 2)
        .unwrap()
        .with_groups(1, vec![Some(0); 2])
        .unwrap()
        .with_source_groups(vec![SAMPLER])
        .unwrap()
        .with_releases(
            vec![Trigger::Attack, Trigger::KeyRelease],
            release,
            Default::default(),
        )
        .unwrap();
    let mut rt = runtime(route_single(p).unwrap());
    support::without_heap(|| {
        let note = rt.trigger(input(1, 60), 60, 1.).unwrap();
        assert_eq!(rt.voice_count(), 1);
        rt.key_up(note, None).unwrap();
        let mut output = [[0.; 2]; 16];
        rt.render(&mut output).unwrap();
        assert!(
            output.iter().any(|f| f[0] != 0.),
            "ordinary release sounds after the routed attack releases"
        );
        assert_eq!(rt.voice_count(), 0);
    });
}

#[test]
fn zone_commit_precedes_deferred_attack_final_admission_with_explicit_capacity_diagnostic() {
    // This is KONTRA's initial ordering policy, not a native callback-order oracle.
    let mut code = vec![Instruction::SetLocal {
        local: 0,
        value: i64::from(ZONE),
    }];
    for (index, value) in [i64::from(WT), 0, 127].into_iter().enumerate() {
        code.extend([
            Instruction::SetLocal {
                local: 1,
                value: i64::from(SELECTORS[index]),
            },
            Instruction::SetLocal { local: 2, value },
            Instruction::Op(Op::SetZoneParameter {
                zone: 0,
                parameter: 1,
                value: 2,
                result: 3 + index as u16,
                selectors: SELECTORS,
                present: 7,
            }),
        ]);
    }
    code.extend([
        Instruction::SetLocal {
            local: 6,
            value: 49,
        },
        Instruction::SetLocal {
            local: 7,
            value: 127,
        },
        Instruction::PlayMidi {
            key: 6,
            velocity: 7,
            duration: DurationValue::Fixed(Duration::Frames(16)),
            offset_micros: None,
            inheritance: Inheritance::Independent,
            result: Some(8),
        },
        Instruction::End,
    ]);
    for voices in [0, 1] {
        let p = make(true, false)
            .with_programs(
                vec![
                    Program::new(code.clone())
                        .unwrap()
                        .with_script_instance(ScriptInstanceId(0))
                        .with_wait_lifetime(WaitLifetime::Callback),
                ],
                None,
            )
            .unwrap()
            .with_controls(vec![ControlDefinition {
                id: ControlId(700),
                domain: ControlDomain::Integer { min: 0, max: 1 },
                default: ControlValue::Integer(0),
            }])
            .unwrap()
            .with_control_programs(vec![ControlCallback {
                control: ControlId(700),
                program: 0,
                stage: 0,
            }])
            .unwrap();
        let limits = Limits::for_plan(&p, 4, voices);
        let mut rt = Runtime::new(p, limits).unwrap();
        support::without_heap(|| {
            // A generated note needs a genuine control/performance origin,
            // rather than the deliberately context-free plan entry point.
            let (_, id) = rt
                .invoke_control(
                    ControlContext {
                        performance: rt.performance(0).unwrap(),
                        origin: ChannelAddress {
                            protocol: Protocol::Native,
                            port: 0,
                            group: 0,
                            channel: 0,
                        },
                        channels: 1,
                    },
                    rt.active_plan(),
                    None,
                    ControlWrite {
                        id: ControlId(700),
                        value: ControlValue::Integer(1),
                    },
                )
                .unwrap();
            let id = id.unwrap();
            assert!(
                rt.behavior_local(id, 8).unwrap() > 0,
                "logical play_note was admitted before the batch boundary"
            );
            assert_eq!(
                rt.zone_parameter_in(rt.active_plan(), ZONE, ZoneParameter::Group),
                Ok(WT as i32)
            );
            assert_eq!(rt.voice_count(), voices);
            assert_eq!(rt.stats().refused_starts, u64::from(voices == 0));
            assert_eq!(
                rt.zone_routing_report(rt.active_plan())
                    .unwrap()
                    .unwrap()
                    .pending_jobs,
                0
            );
            rt.render(&mut [[0.; 2]; 8]).unwrap();
            assert_eq!(rt.now(), 8);
        });
    }
}

/// A complete ordinary lower bank with sparse physical IDs, two destinations
/// per source, and no identity derived from assets or dense region indices.
fn source_sized_plan(count: usize) -> Prepared {
    use sampler_ir as ir;
    let mut instrument = ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("authored-batch.wav".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: vec![],
        }],
        groups: vec![ir::Group::default(), ir::Group::default()],
        zone_routing_mode: Some(3),
        ..Default::default()
    };
    instrument.source_indices.groups = vec![Some(ir::GroupRef(0)), Some(ir::GroupRef(1))];
    for index in 0..count {
        let id = (index * 11) as u32;
        let primary = instrument.zones.len();
        for group in 0..2 {
            instrument.zones.push(ir::Zone {
                group: Some(ir::GroupRef(group)),
                pitch: ir::KeyTracking::Fixed,
                keys: ir::KeyRange { low: 40, high: 80 },
                ..ir::Zone::new(ir::AssetRef(0))
            });
        }
        instrument
            .source_indices
            .zones
            .push(Some(ir::ZoneRef(primary)));
        instrument.source_indices.zone_catalog.push(ir::SourceZone {
            id: Some(id),
            home: ir::SourceZoneHome {
                group: 0,
                low_key: 40,
                high_key: 80,
            },
        });
        instrument.zone_routing.push(ir::ZoneRouting {
            source_zone: id,
            group: 0,
            low_key: 40,
            high_key: 80,
            variants: vec![
                ir::ZoneRouteVariant {
                    group: 0,
                    zone: Some(ir::ZoneRef(primary)),
                },
                ir::ZoneRouteVariant {
                    group: 1,
                    zone: Some(ir::ZoneRef(primary + 1)),
                },
            ],
        });
    }
    sampler_core::lower::lower_with(
        &instrument,
        48_000,
        vec![Pcm::new(48_000, vec![[0.125; 2]; 128].into_boxed_slice()).unwrap()],
        &sampler_core::lower::Options { mpe: None },
        |_, p| Ok(p),
    )
    .unwrap()
}

#[test]
fn source_sized_lower_batch_over_256_keeps_boundary_exact_ids_callbacks_and_no_heap() {
    let count = 100;
    let jobs = source_zone_job_capacity(count).unwrap();
    assert_eq!(jobs, 300);
    let ids = ScriptArray {
        offset: 16,
        len: jobs as u32,
    };
    let statuses = ScriptArray {
        offset: 16 + jobs as u32,
        len: jobs as u32,
    };
    let callback = |instance, code| {
        Program::new(code)
            .unwrap()
            .with_script_instance(ScriptInstanceId(instance))
            .with_wait_lifetime(WaitLifetime::Callback)
    };
    let mut code = Vec::new();
    for index in 0..count {
        code.push(Instruction::SetLocal {
            local: 0,
            value: (index * 11) as i64,
        });
        // Low100 temporarily exceeds old high80. Only the final home publishes.
        for (selector, value) in SELECTORS.into_iter().zip([1, 100, 110]) {
            code.extend([
                Instruction::SetLocal {
                    local: 1,
                    value: i64::from(selector),
                },
                Instruction::SetLocal { local: 2, value },
                Instruction::Op(Op::SetZoneParameter {
                    zone: 0,
                    parameter: 1,
                    value: 2,
                    result: 3,
                    selectors: SELECTORS,
                    present: 7,
                }),
            ]);
        }
    }
    for (index, selector) in SELECTORS.into_iter().enumerate() {
        code.extend([
            Instruction::SetLocal {
                local: 1,
                value: i64::from(selector),
            },
            Instruction::Op(Op::ZoneParameter {
                zone: 0,
                parameter: 1,
                selectors: SELECTORS,
                present: 7,
                local: 4,
            }),
            Instruction::WriteScriptCell {
                cell: 1 + index as u32,
                local: 4,
            },
        ]);
    }
    code.extend([
        Instruction::WriteScriptCell { cell: 7, local: 3 },
        Instruction::Op(Op::WaitMidi { local: 3 }),
    ]);
    for (index, selector) in SELECTORS.into_iter().enumerate() {
        code.extend([
            Instruction::SetLocal {
                local: 1,
                value: i64::from(selector),
            },
            Instruction::Op(Op::ZoneParameter {
                zone: 0,
                parameter: 1,
                selectors: SELECTORS,
                present: 7,
                local: 4,
            }),
            Instruction::WriteScriptCell {
                cell: 4 + index as u32,
                local: 4,
            },
        ]);
    }
    code.push(Instruction::End);
    let plan = source_sized_plan(count)
        .with_script_instances(vec![vec![0; 16 + jobs * 2], vec![0]])
        .unwrap()
        .with_programs(
            vec![
                callback(0, code),
                callback(
                    1,
                    vec![
                        Instruction::SetLocal {
                            local: 0,
                            value: -99,
                        },
                        Instruction::WriteScriptCell { cell: 0, local: 0 },
                        Instruction::End,
                    ],
                ),
                callback(
                    0,
                    vec![
                        Instruction::Op(Op::ReadHost { local: 0, slot: 6 }),
                        Instruction::Op(Op::ReadHost { local: 1, slot: 7 }),
                        Instruction::ReadScriptCell { local: 2, cell: 0 },
                        Instruction::WriteScriptArray {
                            array: ids,
                            index: 2,
                            local: 0,
                        },
                        Instruction::WriteScriptArray {
                            array: statuses,
                            index: 2,
                            local: 1,
                        },
                        Instruction::SetLocal { local: 3, value: 1 },
                        Instruction::Binary32 {
                            lhs: 2,
                            rhs: 3,
                            operation: IntegerBinary::Add,
                        },
                        Instruction::WriteScriptCell { cell: 0, local: 2 },
                        Instruction::End,
                    ],
                ),
            ],
            None,
        )
        .unwrap()
        .with_signal_programs(vec![
            SignalProgram {
                signal: MIDI_ASYNC_SIGNAL,
                program: 1,
                stage: 0,
            },
            SignalProgram {
                signal: MIDI_ASYNC_SIGNAL,
                program: 2,
                stage: 0,
            },
        ])
        .unwrap();
    assert_eq!(plan.zone_routing_job_capacity(), jobs);
    let limits = Limits::for_plan(&plan, 4, 4);
    assert!(limits.behaviors >= jobs + 16);
    assert!(limits.behavior_cells <= Limits::SCRIPT_CELLS);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let active = rt.active_plan();
    support::without_heap(|| {
        // The entire source-sized transaction crosses a fuel boundary with
        // all300 jobs pending. Preemption must not publish an early home.
        rt.begin_behavior_block(count * 10, None);
        rt.start_plan_behavior(active, 0).unwrap();
        let report = rt.zone_routing_report(active).unwrap().unwrap();
        assert_eq!((report.job_capacity, report.pending_jobs), (jobs, jobs));
        assert_eq!(report.refusals, [0; 8]);
        for index in 0..count {
            assert_eq!(
                rt.zone_parameter_in(active, (index * 11) as u32, ZoneParameter::Group),
                Ok(0)
            );
            assert_eq!(
                rt.zone_parameter_in(active, (index * 11) as u32, ZoneParameter::LowKey),
                Ok(40)
            );
        }
        rt.begin_behavior_block(20_000, None);
        rt.render(&mut [[0.; 2]; 1]).unwrap();
        for (cell, expected) in [
            (0, 300),
            (1, 0),
            (2, 40),
            (3, 80),
            (4, 1),
            (5, 100),
            (6, 110),
            (7, 300),
        ] {
            assert_eq!(
                rt.script_cell(active, ScriptInstanceId(0), cell),
                Ok(expected)
            );
        }
        for index in 0..jobs as u32 {
            assert_eq!(
                rt.script_cell(active, ScriptInstanceId(0), ids.offset + index),
                Ok(i64::from(index + 1))
            );
            assert_eq!(
                rt.script_cell(active, ScriptInstanceId(0), statuses.offset + index),
                Ok(1)
            );
        }
        assert_eq!(rt.script_cell(active, ScriptInstanceId(1), 0), Ok(0));
        for index in 0..count {
            let zone = (index * 11) as u32;
            for (parameter, value) in [
                (ZoneParameter::Group, 1),
                (ZoneParameter::LowKey, 100),
                (ZoneParameter::HighKey, 110),
            ] {
                assert_eq!(rt.zone_parameter_in(active, zone, parameter), Ok(value));
            }
        }
        let report = rt.zone_routing_report(active).unwrap().unwrap();
        assert_eq!(report.pending_jobs, 0);
        assert_eq!(report.refusals, [0; 8]);
        assert_eq!(rt.take_fault(), None);
        rt.flush_behaviors(|_, _, _| true);
    });
}

#[test]
fn a_shared_source_zone_identity_stays_on_every_zone_and_addresses_the_first() {
    let row = |id, group, region| SourceZoneIdentity {
        id: Some(id),
        home: SourceZoneHome { group, low_key: 60, high_key: 61 },
        region,
    };
    let p = make(false, false)
        .with_source_zone_identities(vec![row(5, SAMPLER, Some(0)), row(5, WT, None), row(9, WT, None)], 4, 4096)
        .unwrap();
    assert_eq!([0, 1, 2].map(|i| p.source_zone_identity(i)), [Some(5), Some(5), Some(9)]);
    assert_eq!(p.source_zone_identity_home(5).map(|h| h.group), Some(SAMPLER));
    assert_eq!(p.source_zone_identity_region(5), Some(0));
}
