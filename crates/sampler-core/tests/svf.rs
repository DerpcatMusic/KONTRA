use sampler_core::*;
mod support;
const CUTOFF: ControlId = ControlId(13);
const Q: ControlId = ControlId(14);

fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 4,
        voices: 4,
        expressions: 4,
        decisions: 0,
        commands: 8,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
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
fn source(i: usize) -> Frame {
    if i >= 160 {
        [0.; 2]
    } else {
        [(i as f32 * 0.07).sin(), if i == 0 { 1. } else { 0. }]
    }
}
fn plan(rate: u32, filter: StateVariableFilter, shared: bool) -> Result<Prepared, Error> {
    let plan = base(rate)?;
    if shared {
        plan.with_buses(
            vec![Bus {
                processors: vec![Processor::StateVariable(filter)],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 40,
            }],
            vec![Some(0)],
        )
    } else {
        plan.with_voice_chains(
            vec![VoiceChain::new(
                vec![],
                vec![Processor::StateVariable(filter)],
                40,
            )?],
            vec![Some(0)],
        )
    }
}
fn base(rate: u32) -> Result<Prepared, Error> {
    Prepared::new(
        rate,
        vec![Pcm::new(rate, (0..160).map(source).collect())?],
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
    )?
    .with_controls(vec![
        ControlDefinition {
            id: CUTOFF,
            domain: ControlDomain::Real { min: 0., max: 1. },
            default: ControlValue::Real(0.),
        },
        ControlDefinition {
            id: Q,
            domain: ControlDomain::Real { min: 0., max: 1. },
            default: ControlValue::Real(0.),
        },
    ])
}
fn filter(mode: SvfMode) -> StateVariableFilter {
    StateVariableFilter {
        mode,
        cutoff_hz: Parameter::Control(ControlRange {
            control: CUTOFF,
            low: 1000.,
            high: 9000.,
            ramp_frames: 8,
        }),
        q: Parameter::Control(ControlRange {
            control: Q,
            low: 0.5,
            high: 2.5,
            ramp_frames: 4,
        }),
    }
}
fn hz(at: usize) -> f64 {
    match at {
        0..=5 => 1000.,
        6..=10 => 1000. + (at - 5) as f64 * 1000.,
        11..=17 => 6000. - (at - 10) as f64 * 375.,
        _ => 3000.,
    }
}
fn q(at: usize) -> f64 {
    match at {
        0..=10 => 0.5,
        11..=13 => 0.5 + (at - 10) as f64 * 0.5,
        14..=80 => 2.5,
        81..=83 => 2.5 - (at - 80) as f64 * 0.5,
        _ => 0.5,
    }
}

// Solve the two implicit trapezoidal integrators through the high-pass node.
// This deliberately avoids the production kernel's a1/a2/a3 recurrence.
fn reference(
    mode: SvfMode,
    rate: u32,
    hz: f64,
    q: f64,
    state: &mut [[f64; 2]; 2],
    frame: Frame,
) -> Frame {
    let g = (std::f64::consts::PI * hz / f64::from(rate)).tan();
    let k = 1. / q;
    std::array::from_fn(|i| {
        let x = f64::from(frame[i]);
        let high = (x - (k + g) * state[i][0] - state[i][1]) / (1. + k * g + g * g);
        let band = state[i][0] + g * high;
        let low = state[i][1] + g * band;
        state[i] = [2. * band - state[i][0], 2. * low - state[i][1]];
        (match mode {
            SvfMode::LowPass => low,
            SvfMode::HighPass => high,
            SvfMode::BandPass => k * band,
            SvfMode::Notch => low + high,
            SvfMode::AllPass => low + high - k * band,
            SvfMode::OnePoleLowPass | SvfMode::OnePoleHighPass => unreachable!("not in this list"),
        }) as f32
    })
}
fn near(actual: Frame, expected: Frame) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 3e-6, "{actual:?} != {expected:?}");
    }
}

#[test]
fn automated_filters_share_absolute_parameter_time_but_keep_audio_histories_separate() {
    for mode in [
        SvfMode::LowPass,
        SvfMode::HighPass,
        SvfMode::BandPass,
        SvfMode::Notch,
        SvfMode::AllPass,
    ] {
        for shared in [false, true] {
            let mut expected = [[0.; 2]; 220];
            let mut histories = [[[0.; 2]; 2]; 2];
            for (at, output) in expected.iter_mut().enumerate().take(203) {
                let first = source(at);
                let second = if at >= 3 {
                    source(at - 3).map(|v| v * 0.5)
                } else {
                    [0.; 2]
                };
                if shared {
                    *output = reference(
                        mode,
                        48000,
                        hz(at),
                        q(at),
                        &mut histories[0],
                        [first[0] + second[0], first[1] + second[1]],
                    );
                } else {
                    if at < 200 {
                        *output = reference(mode, 48000, hz(at), q(at), &mut histories[0], first);
                    }
                    if at >= 3 {
                        let value =
                            reference(mode, 48000, hz(at), q(at), &mut histories[1], second);
                        for i in 0..2 {
                            output[i] += value[i];
                        }
                    }
                }
            }
            for block in [1, 7, 64, 129] {
                let mut rt =
                    Runtime::new(plan(48000, filter(mode), shared).unwrap(), limits()).unwrap();
                let mut actual = [[0.; 2]; 220];
                support::without_heap(|| {
                    let generation = rt.active_plan();
                    rt.trigger(input(1), 60, 1.).unwrap();
                    for (at, id, value) in [
                        (5, CUTOFF, 1.),
                        (10, CUTOFF, 0.25),
                        (10, Q, 1.),
                        (80, Q, 0.),
                    ] {
                        rt.schedule_event(
                            at,
                            Event::Control(
                                generation,
                                ControlWrite {
                                    id,
                                    value: ControlValue::Real(value),
                                },
                            ),
                        )
                        .unwrap();
                    }
                    rt.render(&mut actual[..3]).unwrap();
                    rt.trigger(input(2), 60, 0.5).unwrap();
                    for chunk in actual[3..].chunks_mut(block) {
                        rt.render(chunk).unwrap();
                    }
                    for (a, b) in actual.into_iter().zip(expected) {
                        near(a, b);
                    }
                    assert_eq!(rt.nonfinite_frames(), 0);
                    rt.note_off(input(1), None).unwrap();
                    rt.note_off(input(2), None).unwrap();
                    rt.flush_ended(|_| true);
                    assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
                });
            }
        }
    }
}

#[test]
fn replacement_retains_old_filter_trajectories_and_independent_new_histories() {
    let (mut rt, mut transfer) = Runtime::with_plan_updates(
        plan(48000, filter(SvfMode::LowPass), false).unwrap(),
        limits(),
        2,
        1,
    )
    .unwrap();
    let old = rt.active_plan();
    let replacement = StateVariableFilter {
        mode: SvfMode::HighPass,
        cutoff_hz: Parameter::Constant(6000.),
        q: Parameter::Constant(1.5),
    };
    let request = transfer
        .submit(Box::new(plan(48000, replacement, false).unwrap()))
        .unwrap();
    let mut old_history = [[0.; 2]; 2];
    let mut new_history = [[0.; 2]; 2];
    let mut expected = [[0.; 2]; 208];
    for (at, output) in expected.iter_mut().enumerate() {
        if at < 200 {
            let frequency = if at <= 12 {
                1000.
            } else if at < 20 {
                1000. + (at - 12) as f64 * 1000.
            } else {
                9000.
            };
            *output = reference(
                SvfMode::LowPass,
                48000,
                frequency,
                0.5,
                &mut old_history,
                source(at),
            );
        }
        if at >= 8 {
            let new = reference(
                SvfMode::HighPass,
                48000,
                6000.,
                1.5,
                &mut new_history,
                source(at - 8),
            );
            for i in 0..2 {
                output[i] += new[i];
            }
        }
    }
    support::without_heap(|| {
        rt.trigger(input(1), 60, 1.).unwrap();
        rt.schedule_event(
            12,
            Event::Control(
                old,
                ControlWrite {
                    id: CUTOFF,
                    value: ControlValue::Real(1.),
                },
            ),
        )
        .unwrap();
        let mut actual = [[0.; 2]; 208];
        rt.render(&mut actual[..8]).unwrap();
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.trigger(input(2), 60, 1.).unwrap();
        rt.render(&mut actual[8..]).unwrap();
        for (a, b) in actual.into_iter().zip(expected) {
            near(a, b);
        }
        assert_eq!(rt.control_value(old, CUTOFF), Ok(ControlValue::Real(1.)));
        assert_eq!(
            rt.control_value(rt.active_plan(), CUTOFF),
            Ok(ControlValue::Real(0.))
        );
        rt.note_off(input(1), None).unwrap();
        rt.note_off(input(2), None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
    });
    drop(transfer.retired().unwrap());
}

#[test]
fn note_owned_pressure_and_timbre_follow_link_snapshot_detach_and_slot_reuse() {
    let filter = StateVariableFilter {
        mode: SvfMode::LowPass,
        cutoff_hz: Parameter::Expression {
            source: ExpressionSource::Timbre,
            low: 1000.,
            high: 9000.,
        },
        q: Parameter::Expression {
            source: ExpressionSource::Pressure,
            low: 0.5,
            high: 2.5,
        },
    };
    // A summed signal has no unique expression owner. Never silently pick one.
    assert!(matches!(
        plan(48000, filter, true),
        Err(Error::InvalidInput)
    ));
    let mut excessive = limits();
    excessive.expressions = usize::MAX;
    assert!(matches!(
        Runtime::new(plan(48000, filter, false).unwrap(), excessive),
        Err(Error::Capacity)
    ));
    for block in [1, 7, 64, 129] {
        let prepared = plan(48000, filter, false).unwrap();
        let (mut rt, mut transfer) = Runtime::with_plan_updates(prepared, limits(), 2, 1).unwrap();
        let replacement = transfer
            .submit(Box::new(plan(48000, filter, false).unwrap()))
            .unwrap();
        let initial = Expression::default();
        let high = Expression {
            pressure: u32::MAX,
            timbre: u32::MAX,
            ..initial
        };
        let middle = Expression {
            pressure: u32::MAX - 1,
            timbre: u32::MAX / 2,
            ..initial
        };
        let mut expected = [[0.; 2]; 220];
        let mut histories = [[[0.; 2]; 2]; 4];
        for (at, output) in expected.iter_mut().enumerate().take(200) {
            let root = if (4..8).contains(&at) { high } else { initial };
            let linked = if at < 4 {
                initial
            } else if at < 12 {
                high
            } else {
                middle
            };
            for (state, expression) in histories.iter_mut().zip([root, linked, initial, high]) {
                let frequency = 1000. + 8000. * f64::from(expression.timbre) / f64::from(u32::MAX);
                let quality = 0.5 + 2. * f64::from(expression.pressure) / f64::from(u32::MAX);
                let value = reference(
                    SvfMode::LowPass,
                    48000,
                    frequency,
                    quality,
                    state,
                    source(at),
                );
                for i in 0..2 {
                    output[i] += value[i];
                }
            }
        }
        support::without_heap(|| {
            let root = rt.trigger(input(1), 60, 1.).unwrap();
            let linked = rt.child(root, 60, 1., false, Inheritance::Linked).unwrap();
            assert!(rt.forward_attack(linked).unwrap());
            let snapshot = rt
                .child(root, 60, 1., false, Inheritance::Snapshot)
                .unwrap();
            assert!(rt.forward_attack(snapshot).unwrap());
            rt.trigger_with_expression(input(2), 60, 1., high).unwrap();
            let root_expression = rt.expression_id(root).unwrap();
            assert_eq!(rt.expression_id(linked), Ok(root_expression));
            let mut actual = [[0.; 2]; 220];
            rt.render(&mut actual[..4]).unwrap();
            rt.set_expression(root_expression, high).unwrap();
            rt.render(&mut actual[4..8]).unwrap();
            let detached = rt.detach_expression(linked).unwrap();
            assert_ne!(detached, root_expression);
            rt.set_expression(root_expression, initial).unwrap();
            rt.render(&mut actual[8..12]).unwrap();
            rt.set_expression(detached, middle).unwrap();
            for chunk in actual[12..].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (a, b) in actual.into_iter().zip(expected) {
                near(a, b);
            }
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(rt.expression_count(), 0);
            // Reuse an expression slot in the same retained filter bank.
            let reused = rt.trigger_with_expression(input(3), 60, 1., high).unwrap();
            assert_ne!(rt.expression_id(reused), Ok(root_expression));
            let mut state = [[0.; 2]; 2];
            let mut audio = [[0.; 2]; 20];
            rt.render(&mut audio).unwrap();
            for (at, value) in audio.into_iter().enumerate() {
                near(
                    value,
                    reference(SvfMode::LowPass, 48000, 9000., 2.5, &mut state, source(at)),
                );
            }
            rt.panic();
            rt.flush_ended(|_| true);
            // Replacement also prepared the expression capacity off audio.
            assert_eq!(rt.poll_plan_update(), Ok(Some(replacement)));
            rt.trigger_with_expression(input(4), 60, 1., high).unwrap();
            let mut first = [[0.; 2]; 1];
            rt.render(&mut first).unwrap();
            near(
                first[0],
                reference(
                    SvfMode::LowPass,
                    48000,
                    9000.,
                    2.5,
                    &mut [[0.; 2]; 2],
                    source(0),
                ),
            );
            assert_eq!(rt.plan_count(), 1);
            assert_eq!(rt.collect_retired_plans(), 0);
        });
        drop(transfer.retired().unwrap());
    }
}

#[test]
fn static_filter_boundaries_rates_and_invalid_control_ranges_are_explicit() {
    for rate in [44100, 48000, 96000] {
        for cutoff in [10., f64::from(rate) * 0.1, f64::from(rate) * 0.499] {
            for quality in [0.1, 0.707, 20.] {
                let mut filter = filter(SvfMode::LowPass);
                filter.cutoff_hz = Parameter::Constant(cutoff);
                filter.q = Parameter::Constant(quality);
                let mut rt = Runtime::new(plan(rate, filter, false).unwrap(), limits()).unwrap();
                let mut state = [[0.; 2]; 2];
                let expected: Vec<_> = (0..200)
                    .map(|i| {
                        reference(
                            SvfMode::LowPass,
                            rate,
                            cutoff,
                            quality,
                            &mut state,
                            source(i),
                        )
                    })
                    .collect();
                support::without_heap(|| {
                    rt.trigger(input(1), 60, 1.).unwrap();
                    let mut output = [[0.; 2]; 200];
                    rt.render(&mut output).unwrap();
                    for (a, b) in output.into_iter().zip(&expected) {
                        near(a, *b);
                    }
                });
            }
        }
    }
    for (cutoff, quality) in [
        (0., 1.),
        (24000., 1.),
        (f64::NAN, 1.),
        (1., 0.),
        (1000., f64::INFINITY),
        (1000., f64::from_bits(1)),
    ] {
        let invalid = StateVariableFilter {
            mode: SvfMode::LowPass,
            cutoff_hz: Parameter::Constant(cutoff),
            q: Parameter::Constant(quality),
        };
        assert!(plan(48000, invalid, false).is_err());
    }
    let mut invalid = filter(SvfMode::LowPass);
    invalid.cutoff_hz = Parameter::Control(ControlRange {
        control: CUTOFF,
        low: 20000.,
        high: 25000.,
        ramp_frames: 0,
    });
    assert!(plan(48000, invalid, true).is_err());
    let mut invalid = filter(SvfMode::LowPass);
    invalid.q = Parameter::Control(ControlRange {
        control: ControlId(999),
        low: 0.5,
        high: 2.,
        ramp_frames: 0,
    });
    assert!(plan(48000, invalid, false).is_err());
    assert!(
        plan(48000, filter(SvfMode::LowPass), false)
            .unwrap()
            .with_controls(vec![])
            .is_err()
    );
}

// Voices sharing a chain render in lanes; each must match rendering alone,
// bit for bit, across batch boundaries, staggered starts and ended voices.
#[test]
fn batched_voices_match_voices_rendered_alone() {
    const VOICES: usize = 11;
    let native_parameter = |low, high| {
        Parameter::Control(ControlRange {
            control: CUTOFF,
            low,
            high,
            ramp_frames: 4,
        })
    };
    let chain = || {
        VoiceChain::new(
            vec![
                Processor::Gainer {
                    dry: 0.1,
                    gain: native_parameter(0.2, 2.0),
                },
                Processor::StereoMatrix([[0.9, 0.2], [-0.1, 1.1]]),
                Processor::Biquad(Biquad::new(48000, FilterKind::LowPass, 7000., 0.8).unwrap()),
                Processor::StateVariable(filter(SvfMode::LowPass)),
            ],
            vec![
                Processor::Gain(0.7),
                Processor::StereoModeller(StereoSettings {
                    width: native_parameter(0.0, 1.0),
                    pan: native_parameter(-0.8, 0.5),
                    pseudo: false,
                }),
                Processor::StateVariable(filter(SvfMode::BandPass)),
            ],
            40,
        )
        .unwrap()
    };
    let limits = Limits {
        notes: VOICES,
        families: VOICES,
        voices: VOICES,
        expressions: VOICES,
        ..limits()
    };
    let render = |voices: std::ops::Range<usize>| {
        let plan = base(48000)
            .unwrap()
            .with_voice_chains(vec![chain()], vec![Some(0)])
            .unwrap();
        let mut rt = Runtime::new(plan, limits).unwrap();
        let generation = rt.active_plan();
        rt.schedule_event(
            20,
            Event::Control(
                generation,
                ControlWrite {
                    id: CUTOFF,
                    value: ControlValue::Real(1.),
                },
            ),
        )
        .unwrap();
        let mut out = [[0.; 2]; 300];
        let mut at = 0;
        for (k, chunk) in [5, 64, 1, 64, 30, 64, 64, 8].into_iter().enumerate() {
            for v in voices.clone() {
                if v % 4 == k {
                    rt.trigger(input(v as i32), 60, 0.2 + v as f64 * 0.07)
                        .unwrap();
                }
                if v % 3 == 0 && k == 4 {
                    rt.note_off(input(v as i32), None).unwrap();
                }
            }
            support::without_heap(|| rt.render(&mut out[at..at + chunk]).unwrap());
            at += chunk;
        }
        out
    };
    let together = render(0..VOICES);
    let mut alone = [[0.; 2]; 300];
    // Sum in slot order, which is trigger order here.
    let mut order: Vec<_> = (0..VOICES).collect();
    order.sort_by_key(|v| v % 4);
    for v in order {
        for (a, b) in alone.iter_mut().zip(render(v..v + 1)) {
            *a = [a[0] + b[0], a[1] + b[1]];
        }
    }
    assert_eq!(together, alone);
}
