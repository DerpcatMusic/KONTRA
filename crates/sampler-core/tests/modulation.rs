use sampler_core::{
    Destination, Envelope, Error, Event, Expression, ExpressionSource, Inheritance, Input, Limits,
    Modulation, Pcm, Playback, Prepared, Protocol, Region, Route, Runtime,
};
mod support;

fn limits() -> Limits {
    Limits {
        notes: 8,
        channels: 1,
        performances: 1,
        families: 8,
        decisions: 0,
        expressions: 8,
        voices: 8,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
    }
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn plan(routes: Vec<Route>, playback: Playback, constant: bool) -> Prepared {
    let modulation = Modulation::new(routes, 8).unwrap();
    Prepared::new(
        48000,
        vec![
            Pcm::new(
                48000,
                (0..8192)
                    .map(|i| {
                        if constant {
                            [1.0; 2]
                        } else {
                            let phase = f64::from(i) * std::f64::consts::TAU * 0.017;
                            [phase.cos() as f32, phase.sin() as f32]
                        }
                    })
                    .collect(),
            )
            .unwrap(),
        ],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
            envelope: Envelope::new(0, 0, 0, 1.0, 64).unwrap(),
            playback,
        }],
        1,
    )
    .unwrap()
    .with_modulation(modulation)
}
fn route(source: ExpressionSource, destination: Destination) -> Route {
    Route {
        source,
        destination,
    }
}

#[test]
fn projected_expression_is_sample_exact_and_keeps_raw_values() {
    for partition in [1, 7, 64, 256] {
        let mut rt = Runtime::new(
            plan(
                vec![
                    route(
                        ExpressionSource::Pressure,
                        Destination::LinearGain {
                            zero: 0.25,
                            one: 1.0,
                        },
                    ),
                    route(
                        ExpressionSource::Timbre,
                        Destination::StereoBalance {
                            zero: -0.25,
                            one: 0.25,
                        },
                    ),
                    route(
                        ExpressionSource::Timbre,
                        Destination::PitchSemitones {
                            zero: 0.0,
                            one: 12.0,
                        },
                    ),
                ],
                Playback::default(),
                false,
            ),
            limits(),
        )
        .unwrap();
        let mut reference =
            Runtime::new(plan(vec![], Playback::default(), false), limits()).unwrap();
        let raw = Expression {
            gain: 0.5,
            pan: 0.125,
            pitch_semitones: 3.0,
            pressure: u32::MAX,
            timbre: 0,
        };
        let raw_next = Expression {
            pressure: 0,
            timbre: u32::MAX,
            ..raw
        };
        let golden = Expression {
            gain: 0.5,
            pan: -0.125,
            pitch_semitones: 3.0,
            ..Expression::default()
        };
        let golden_next = Expression {
            gain: 0.125,
            pan: 0.375,
            pitch_semitones: 15.0,
            ..Expression::default()
        };
        let mut actual = [[0.0; 2]; 256];
        let mut expected = actual;
        support::without_heap(|| {
            let note = rt.trigger_with_expression(input(1), 60, 1.0, raw).unwrap();
            let other = reference
                .trigger_with_expression(input(1), 60, 1.0, golden)
                .unwrap();
            assert_eq!(rt.expression(rt.expression_id(note).unwrap()), Ok(raw));
            for (at, value, projected) in [
                (64, raw_next, golden_next),
                (127, raw, golden),
                (129, raw_next, golden_next),
            ] {
                rt.schedule_event(at, Event::Expression(note, value))
                    .unwrap();
                reference
                    .schedule_event(at, Event::Expression(other, projected))
                    .unwrap();
            }
            for (a, b) in actual
                .chunks_mut(partition)
                .zip(expected.chunks_mut(partition))
            {
                rt.render(a).unwrap();
                reference.render(b).unwrap();
            }
            assert_eq!(actual, expected);
            assert_eq!(rt.expression(rt.expression_id(note).unwrap()), Ok(raw_next));
            for runtime in [&mut rt, &mut reference] {
                runtime.panic();
                runtime.flush_ended(|_| true);
                assert_eq!(
                    (
                        runtime.note_count(),
                        runtime.expression_count(),
                        runtime.voice_count()
                    ),
                    (0, 0, 0)
                );
            }
        });
    }
}

#[test]
fn projected_pitch_participates_in_initial_queued_and_batch_source_preflight() {
    let routes = || {
        vec![route(
            ExpressionSource::Pressure,
            Destination::PitchSemitones {
                zero: 0.0,
                one: 12.0,
            },
        )]
    };
    let mut rt = Runtime::new(plan(routes(), Playback::default(), false), limits()).unwrap();
    let mut at_limit = Runtime::new(
        plan(
            routes(),
            Playback {
                transpose_semitones: 48.0,
                ..Playback::default()
            },
            false,
        ),
        limits(),
    )
    .unwrap();
    let full = Expression {
        pressure: u32::MAX,
        ..Expression::default()
    };
    support::without_heap(|| {
        assert_eq!(
            at_limit.trigger_with_expression(input(1), 60, 1.0, full),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            (
                at_limit.note_count(),
                at_limit.expression_count(),
                at_limit.family_count(),
                at_limit.voice_count()
            ),
            (0, 0, 0, 0)
        );
        let note = rt.note_on(input(1), 60, 1.0).unwrap();
        let owner = rt.expression_id(note).unwrap();
        rt.schedule_event(8, Event::Expression(note, full)).unwrap();
        let family = rt.create_family(note).unwrap();
        assert_eq!(
            rt.start_family(
                family,
                0,
                0,
                1.0,
                Envelope::default(),
                Playback {
                    transpose_semitones: 48.0,
                    ..Playback::default()
                }
            ),
            Err(Error::InvalidInput)
        );
        rt.start_family(
            family,
            0,
            0,
            1.0,
            Envelope::default(),
            Playback {
                transpose_semitones: 36.0,
                ..Playback::default()
            },
        )
        .unwrap();
        rt.finish_family(family).unwrap();
        let invalid = Expression {
            pitch_semitones: 1.0,
            ..full
        };
        assert_eq!(rt.set_expression(owner, invalid), Err(Error::InvalidInput));
        assert_eq!(
            rt.set_expressions(&[(owner, invalid)]),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            rt.schedule_event(4, Event::Expression(note, invalid)),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.expression(owner), Ok(Expression::default()));
        assert_eq!(rt.pending_commands(), 1);
        rt.render(&mut [[0.0; 2]; 9]).unwrap();
        assert_eq!(rt.expression(owner), Ok(full));
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!(
            (rt.note_count(), rt.expression_count(), rt.voice_count()),
            (0, 0, 0)
        );
    });
    for destination in [
        Destination::LinearGain {
            zero: -1.0,
            one: 1.0,
        },
        Destination::StereoBalance {
            zero: 0.0,
            one: 2.0,
        },
        Destination::PitchSemitones {
            zero: f64::NAN,
            one: 1.0,
        },
    ] {
        assert!(matches!(
            Modulation::new(vec![route(ExpressionSource::Pressure, destination)], 1),
            Err(Error::InvalidInput)
        ));
    }
    assert!(matches!(Modulation::new(routes(), 0), Err(Error::Capacity)));
}

#[test]
fn linked_snapshot_and_independent_notes_keep_the_original_modulation_plan() {
    let old = plan(
        vec![route(
            ExpressionSource::Pressure,
            Destination::LinearGain {
                zero: 0.0,
                one: 1.0,
            },
        )],
        Playback::default(),
        true,
    );
    let new = plan(
        vec![route(
            ExpressionSource::Pressure,
            Destination::LinearGain {
                zero: 1.0,
                one: 0.0,
            },
        )],
        Playback::default(),
        true,
    );
    let (mut rt, mut control) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
    let request = control.submit(Box::new(new)).unwrap();
    support::without_heap(|| {
        let initial = Expression {
            pan: -1.0,
            pressure: u32::MAX,
            ..Expression::default()
        };
        let root = rt
            .trigger_with_expression(input(1), 60, 1.0, initial)
            .unwrap();
        let snapshot = rt
            .child(root, 60, 1.0, false, Inheritance::Snapshot)
            .unwrap();
        rt.start(snapshot, 0, 0, 1.0).unwrap();
        let linked = rt.child(root, 60, 1.0, false, Inheritance::Linked).unwrap();
        rt.start(linked, 0, 0, 1.0).unwrap();
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        let independent = rt
            .child(root, 60, 1.0, false, Inheritance::Independent)
            .unwrap();
        rt.start(independent, 0, 0, 1.0).unwrap();
        let current = rt
            .trigger_with_expression(
                input(2),
                60,
                1.0,
                Expression {
                    pan: 1.0,
                    ..initial
                },
            )
            .unwrap();
        let mut frame = [[0.0; 2]; 1];
        rt.render(&mut frame).unwrap();
        assert_eq!(frame, [[3.0, 0.0]]);
        rt.set_expressions(&[
            (
                rt.expression_id(root).unwrap(),
                Expression {
                    pressure: 0,
                    ..initial
                },
            ),
            (
                rt.expression_id(current).unwrap(),
                Expression {
                    pressure: 0,
                    pan: 1.0,
                    ..initial
                },
            ),
        ])
        .unwrap();
        rt.render(&mut frame).unwrap();
        assert_eq!(frame, [[1.0, 1.0]]);
        rt.panic();
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(
            (rt.note_count(), rt.expression_count(), rt.voice_count()),
            (0, 0, 0)
        );
    });
    assert!(control.retired().is_some());
}

#[test]
fn adjacent_32_bit_pressure_values_remain_distinct_at_the_audio_destination() {
    let mut rt = Runtime::new(
        plan(
            vec![route(
                ExpressionSource::Pressure,
                Destination::LinearGain {
                    zero: 0.0,
                    one: 1.0,
                },
            )],
            Playback::default(),
            true,
        ),
        limits(),
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt
            .trigger_with_expression(
                input(1),
                60,
                1.0,
                Expression {
                    pressure: 1,
                    ..Expression::default()
                },
            )
            .unwrap();
        let owner = rt.expression_id(note).unwrap();
        let mut audio = [[0.0; 2]; 1];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[(1.0 / f64::from(u32::MAX)) as f32; 2]]);
        rt.set_expression(
            owner,
            Expression {
                pressure: 2,
                ..Expression::default()
            },
        )
        .unwrap();
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[(2.0 / f64::from(u32::MAX)) as f32; 2]]);
        assert_eq!(rt.expression(owner).unwrap().pressure, 2);
        rt.panic();
        rt.flush_ended(|_| true);
    });
}

#[test]
fn destination_saturation_and_nonfinite_projection_are_explicit() {
    let mut balance = Runtime::new(
        plan(
            vec![route(
                ExpressionSource::Pressure,
                Destination::StereoBalance {
                    zero: -1.0,
                    one: 1.0,
                },
            )],
            Playback::default(),
            true,
        ),
        limits(),
    )
    .unwrap();
    let mut overflow = Runtime::new(
        plan(
            vec![route(
                ExpressionSource::Pressure,
                Destination::PitchSemitones {
                    zero: f64::MAX,
                    one: f64::MAX,
                },
            )],
            Playback::default(),
            true,
        ),
        limits(),
    )
    .unwrap();
    support::without_heap(|| {
        let note = balance
            .trigger_with_expression(
                input(1),
                60,
                1.0,
                Expression {
                    pan: 0.75,
                    pressure: u32::MAX,
                    ..Expression::default()
                },
            )
            .unwrap();
        let mut frame = [[0.0; 2]; 1];
        balance.render(&mut frame).unwrap();
        assert_eq!(frame, [[0.0, 1.0]]);
        let owner = balance.expression_id(note).unwrap();
        assert_eq!(balance.expression(owner).unwrap().pan, 0.75);
        balance
            .set_expression(
                owner,
                Expression {
                    pan: -0.75,
                    ..Expression::default()
                },
            )
            .unwrap();
        balance.render(&mut frame).unwrap();
        assert_eq!(frame, [[1.0, 0.0]]);
        assert_eq!(
            overflow.note_on_with_expression(
                input(1),
                60,
                1.0,
                Expression {
                    pitch_semitones: f64::MAX,
                    ..Expression::default()
                }
            ),
            Err(Error::InvalidInput)
        );
        assert_eq!((overflow.note_count(), overflow.expression_count()), (0, 0));
        balance.panic();
        balance.flush_ended(|_| true);
    });
}
