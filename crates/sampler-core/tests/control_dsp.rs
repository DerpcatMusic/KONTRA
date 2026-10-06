use sampler_core::*;
mod support;
const LEVEL: ControlId = ControlId(9);
const MUTE: ControlId = ControlId(10);

fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 4,
        expressions: 4,
        voices: 4,
        decisions: 0,
        commands: 4,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
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
fn plan(domain: ControlDomain, default: ControlValue, ramp_frames: u32) -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[1.; 2]; 256])).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_controls(vec![
        ControlDefinition {
            id: MUTE,
            domain: ControlDomain::Toggle,
            default: ControlValue::Toggle(false),
        },
        ControlDefinition {
            id: LEVEL,
            domain,
            default,
        },
    ])
    .unwrap()
    .with_voice_chains(
        vec![
            VoiceChain::new(
                vec![],
                vec![
                    VoiceProcessor::ControlGain(GainControl {
                        control: LEVEL,
                        low: 0.,
                        high: 1.,
                        ramp_frames,
                    }),
                    VoiceProcessor::ControlGain(GainControl {
                        control: MUTE,
                        low: 1.,
                        high: 0.,
                        ramp_frames: 0,
                    }),
                ],
                0,
            )
            .unwrap(),
        ],
        vec![Some(0)],
    )
    .unwrap()
}
fn write(id: ControlId, value: ControlValue) -> ControlWrite {
    ControlWrite { id, value }
}

#[test]
fn ramps_share_absolute_time_across_voices_retarget_smoothly_and_reject_batches_atomically() {
    for block in [1, 3, 7, 64] {
        let mut rt = Runtime::new(
            plan(
                ControlDomain::Integer { min: 0, max: 100 },
                ControlValue::Integer(0),
                8,
            ),
            limits(),
        )
        .unwrap();
        support::without_heap(|| {
            let plan = rt.active_plan();
            rt.trigger(input(1), 60, 1.).unwrap();
            rt.edit_controls(plan, None, &[write(LEVEL, ControlValue::Integer(100))])
                .unwrap();
            let mut start = [[0.; 2]; 2];
            rt.render(&mut start).unwrap();
            assert_eq!(start, [[0.; 2], [0.125; 2]]);
            rt.trigger(input(2), 60, 1.).unwrap();
            rt.render(&mut start).unwrap();
            assert_eq!(
                start,
                [[0.5; 2], [0.75; 2]],
                "new voices join the existing trajectory"
            );
            assert_eq!(
                rt.edit_controls(
                    plan,
                    None,
                    &[
                        write(LEVEL, ControlValue::Integer(0)),
                        write(MUTE, ControlValue::Integer(1)),
                    ]
                ),
                Err(Error::InvalidInput)
            );
            assert_eq!(
                rt.control_value(plan, LEVEL),
                Ok(ControlValue::Integer(100))
            );
            assert_eq!(
                rt.edit_controls(plan, Some(0), &[write(LEVEL, ControlValue::Integer(0))]),
                Err(Error::RevisionConflict)
            );
            let mut frame = [[0.; 2]];
            rt.render(&mut frame).unwrap();
            assert_eq!(frame, [[1.; 2]], "rejected writes do not retarget the ramp");
            // At t=5 the existing gain is 5/8. Reverse from that point, not the old target.
            rt.edit_controls(plan, None, &[write(LEVEL, ControlValue::Integer(0))])
                .unwrap();
            rt.render(&mut frame).unwrap();
            assert_eq!(frame, [[1.25; 2]]);
            rt.edit_controls(plan, None, &[write(LEVEL, ControlValue::Integer(0))])
                .unwrap();
            let mut rest = [[0.; 2]; 80];
            for chunk in rest.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (i, actual) in rest.iter().enumerate() {
                let expected = if i < 7 {
                    1.25 * (7 - i) as f32 / 8.
                } else {
                    0.
                };
                assert_eq!(
                    *actual, [expected; 2],
                    "{block}, {i}; repeated targets must not restart ramps"
                );
            }
            rt.edit_controls(plan, None, &[write(MUTE, ControlValue::Toggle(true))])
                .unwrap();
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}

#[test]
fn queued_recall_updates_dsp_and_generation_replacement_keeps_each_controls_owner() {
    let domain = ControlDomain::Real { min: 0., max: 1. };
    let (rt, mut plans) =
        Runtime::with_plan_updates(plan(domain, ControlValue::Real(0.25), 0), limits(), 2, 1)
            .unwrap();
    let (mut rt, mut controls) = rt.with_control_updates(1, 2).unwrap();
    let old = rt.active_plan();
    plans
        .submit(Box::new(plan(domain, ControlValue::Real(0.75), 0)))
        .unwrap();
    controls
        .submit(ControlRequest {
            plan: old,
            expected_revision: Some(0),
            operation: ControlOperation::Recall(Box::from([
                write(LEVEL, ControlValue::Real(0.5)),
                write(MUTE, ControlValue::Toggle(false)),
            ])),
        })
        .unwrap();
    support::without_heap(|| {
        let original = rt.trigger(input(1), 60, 1.).unwrap();
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        rt.trigger(input(2), 60, 1.).unwrap();
        let mut audio = [[0.; 2]];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[1.; 2]]);
        assert_eq!(rt.poll_control_update(), Ok(Some(1)));
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[1.25; 2]]);
        assert_eq!(rt.control_value(new, LEVEL), Ok(ControlValue::Real(0.75)));
        rt.edit_controls(new, None, &[write(MUTE, ControlValue::Toggle(true))])
            .unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.5; 2]]);
        rt.key_up(original, None).unwrap();
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        rt.panic();
        rt.flush_ended(|_| true);
    });
    assert_eq!(controls.reply().unwrap().result, Ok((2, 1)));
    drop(plans.retired().unwrap());
}

#[test]
fn domain_projection_keeps_extreme_ranges_finite_and_schema_changes_validate_bindings() {
    for (domain, initial, changed, before, after) in [
        (
            ControlDomain::Integer {
                min: i64::MIN,
                max: i64::MAX,
            },
            ControlValue::Integer(i64::MIN),
            ControlValue::Integer(i64::MAX),
            0.,
            1.,
        ),
        (
            ControlDomain::Real {
                min: -f64::MAX,
                max: f64::MAX,
            },
            ControlValue::Real(0.),
            ControlValue::Real(f64::MAX),
            0.5,
            1.,
        ),
        (
            ControlDomain::Real {
                min: f64::from_bits(1),
                max: f64::from_bits(2),
            },
            ControlValue::Real(f64::from_bits(1)),
            ControlValue::Real(f64::from_bits(2)),
            0.,
            1.,
        ),
        (
            ControlDomain::Integer { min: 42, max: 42 },
            ControlValue::Integer(42),
            ControlValue::Integer(42),
            0.,
            0.,
        ),
    ] {
        let mut rt = Runtime::new(plan(domain, initial, 0), limits()).unwrap();
        support::without_heap(|| {
            rt.trigger(input(1), 60, 1.).unwrap();
            let mut audio = [[0.; 2]];
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[before; 2]]);
            rt.edit_controls(rt.active_plan(), None, &[write(LEVEL, changed)])
                .unwrap();
            rt.render(&mut audio).unwrap();
            assert_eq!(audio, [[after; 2]]);
            assert_eq!(rt.nonfinite_frames(), 0);
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
    let domain = ControlDomain::Toggle;
    assert!(
        plan(domain, ControlValue::Toggle(false), 0)
            .with_controls(vec![])
            .is_err()
    );
    let missing = VoiceProcessor::ControlGain(GainControl {
        control: ControlId(999),
        low: 0.,
        high: 1.,
        ramp_frames: 8,
    });
    assert!(
        plan(domain, ControlValue::Toggle(false), 0)
            .with_voice_chains(
                vec![VoiceChain::new(vec![missing], vec![], 0).unwrap()],
                vec![Some(0)]
            )
            .is_err()
    );
    for (low, high) in [(f64::NAN, 1.), (0., f64::INFINITY), (-f64::MAX, f64::MAX)] {
        assert!(
            VoiceChain::new(
                vec![VoiceProcessor::ControlGain(GainControl {
                    control: LEVEL,
                    low,
                    high,
                    ramp_frames: 1
                })],
                vec![],
                0
            )
            .is_err()
        );
    }
}

#[test]
fn one_control_drives_multiple_dsp_bindings_and_schema_order_is_not_identity() {
    let bindings = |frames| {
        VoiceProcessor::ControlGain(GainControl {
            control: LEVEL,
            low: 0.,
            high: 1.,
            ramp_frames: frames,
        })
    };
    let prepared = plan(ControlDomain::Toggle, ControlValue::Toggle(false), 0)
        .with_voice_chains(
            vec![VoiceChain::new(vec![bindings(2)], vec![bindings(4)], 0).unwrap()],
            vec![Some(0)],
        )
        .unwrap()
        .with_controls(vec![
            ControlDefinition {
                id: LEVEL,
                domain: ControlDomain::Integer {
                    min: -100,
                    max: 100,
                },
                default: ControlValue::Integer(-100),
            },
            ControlDefinition {
                id: ControlId(0),
                domain: ControlDomain::Toggle,
                default: ControlValue::Toggle(true),
            },
            ControlDefinition {
                id: MUTE,
                domain: ControlDomain::Toggle,
                default: ControlValue::Toggle(false),
            },
        ])
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    support::without_heap(|| {
        rt.trigger(input(1), 60, 1.).unwrap();
        rt.edit_controls(
            rt.active_plan(),
            None,
            &[write(LEVEL, ControlValue::Integer(100))],
        )
        .unwrap();
        let mut audio = [[0.; 2]; 6];
        rt.render(&mut audio).unwrap();
        assert_eq!(
            audio,
            [[0.; 2], [0.125; 2], [0.5; 2], [0.75; 2], [1.; 2], [1.; 2]]
        );
        rt.panic();
        rt.flush_ended(|_| true);
    });
}
