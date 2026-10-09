use sampler_core::{
    Biquad, Envelope, Event, Expression, FilterKind, Input, Limits, Pcm, Playback, Prepared,
    Processor, Protocol, Region, Runtime, VoiceChain,
};
mod support;
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
fn filter() -> Processor {
    Processor::Biquad(Biquad::new(48000, FilterKind::LowPass, 12000., 0.5).unwrap())
}
fn plan(
    pre: Vec<Processor>,
    post: Vec<Processor>,
    tail: u32,
    envelope: Envelope,
    frames: usize,
) -> Prepared {
    let mut pcm = vec![[0.; 2]; frames];
    pcm[0] = [1., -0.5];
    Prepared::new(
        48000,
        vec![Pcm::new(48000, pcm.into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope,
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_voice_chains(
        vec![VoiceChain::new(pre, post, tail).unwrap()],
        vec![Some(0)],
    )
    .unwrap()
}
fn close(actual: [f32; 2], expected: f32) {
    assert!(
        (actual[0] - expected).abs() < 1e-7,
        "{actual:?}, {expected}"
    );
    assert!((actual[1] + expected * 0.5).abs() < 1e-7);
}

#[test]
fn lofi_voice_state_processes_and_recycles_without_audio_heap_calls() {
    let processor = Processor::LoFi(sampler_core::LoFiSettings {
        bits: 0.1,
        frequency: 0.2,
        noise: 0.6,
        color: 0.7,
    });
    let mut rt = Runtime::new(
        plan(vec![processor], vec![], 0, Envelope::default(), 128),
        limits(),
    )
    .unwrap();
    let mut first = [[0.; 2]; 128];
    let mut second = first;
    support::without_heap(|| {
        for (id, output) in [(1, &mut first), (2, &mut second)] {
            let note = rt.trigger(input(id), 60, 1.).unwrap();
            for chunk in output.chunks_mut(7) {
                rt.render(chunk).unwrap();
            }
            rt.key_up(note, None).unwrap();
            rt.flush_ended(|_| true);
        }
    });
    assert_eq!(first, second);
    assert!(first.iter().flatten().all(|v| v.is_finite()));
    assert!(first.iter().flatten().any(|v| v.abs() > 1e-5));
}

#[test]
fn voice_chain_order_tails_muting_and_slot_reuse_are_sample_exact_without_heap() {
    for block in [1, 2, 7, 64] {
        for pre in [true, false] {
            let processor = vec![filter()];
            let (before, after) = if pre {
                (processor, vec![])
            } else {
                (vec![], processor)
            };
            let envelope = Envelope::new(4, 0, 0, 1., 0).unwrap();
            let mut rt = Runtime::new(plan(before, after, 4, envelope, 1), limits()).unwrap();
            support::without_heap(|| {
                let note = rt.trigger(input(1), 60, 1.).unwrap();
                let mut audio = [[0.; 2]; 8];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                for (index, frame) in audio.into_iter().enumerate() {
                    close(
                        frame,
                        if pre && (index == 1 || index == 2) {
                            0.125
                        } else {
                            0.
                        },
                    );
                }
                assert_eq!((rt.voice_count(), rt.family_count()), (0, 0));
                assert!(
                    rt.key_down(note).unwrap(),
                    "DSP EOF cannot release a physical key"
                );
                rt.key_up(note, None).unwrap();
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        }
        let mut rt = Runtime::new(
            plan(vec![], vec![filter()], 4, Envelope::default(), 1),
            limits(),
        )
        .unwrap();
        support::without_heap(|| {
            for id in [1, 2] {
                let note = rt.trigger(input(id), 60, 1.).unwrap();
                let owner = rt.expression_id(note).unwrap();
                rt.set_expression(
                    owner,
                    Expression {
                        gain: 0.,
                        ..Expression::default()
                    },
                )
                .unwrap();
                let mut muted = [[0.; 2]];
                rt.render(&mut muted).unwrap();
                assert_eq!(muted, [[0.; 2]]);
                rt.set_expression(owner, Expression::default()).unwrap();
                rt.key_up(note, None).unwrap();
                assert_eq!(rt.voice_count(), 1, "filter tail owns its voice");
                let mut audio = [[0.; 2]; 4];
                for chunk in audio.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                close(audio[0], 0.5);
                close(audio[1], 0.25);
                close(audio[2], 0.);
                close(audio[3], 0.);
                rt.flush_ended(|_| true);
                assert_eq!(
                    (rt.voice_count(), rt.family_count(), rt.note_count()),
                    (0, 0, 0)
                );
            }
        });
    }
}

#[test]
fn processor_state_and_tails_retain_original_generation_and_faults_do_not_poison_others() {
    let old = plan(vec![], vec![filter()], 4, Envelope::default(), 1);
    let new = plan(
        vec![],
        vec![Processor::Gain(0.5)],
        0,
        Envelope::default(),
        1,
    );
    let (mut rt, mut transfer) = Runtime::with_plan_updates(old, limits(), 2, 1).unwrap();
    transfer.submit(Box::new(new)).unwrap();
    support::without_heap(|| {
        let old = rt.trigger(input(1), 60, 1.).unwrap();
        let mut first = [[0.; 2]];
        rt.render(&mut first).unwrap();
        close(first[0], 0.25);
        rt.key_up(old, None).unwrap();
        assert_eq!(rt.poll_plan_update(), Ok(Some(1)));
        let fresh = rt.trigger(input(2), 60, 1.).unwrap();
        rt.schedule_event(2, Event::KeyUp(fresh, None)).unwrap();
        assert_eq!(rt.collect_retired_plans(), 0);
        let mut audio = [[0.; 2]; 4];
        rt.render(&mut audio).unwrap();
        close(audio[0], 1.);
        close(audio[1], 0.25);
        rt.flush_ended(|_| false);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_ended(|_| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
    });
    drop(transfer.retired().unwrap());
    let mut rt = Runtime::new(
        plan(
            vec![],
            vec![Processor::Gain(f64::MAX), filter()],
            2,
            Envelope::default(),
            1,
        ),
        limits(),
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 8]);
        assert_eq!(rt.nonfinite_frames(), 1);
        rt.key_up(note, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
    });
    assert!(VoiceChain::new(vec![Processor::Gain(f64::NAN)], vec![], 0).is_err());
    let incompatible =
        Processor::Biquad(Biquad::new(96000, FilterKind::LowPass, 1000., 1.).unwrap());
    assert!(
        Prepared::new(48000, vec![], vec![], 0)
            .unwrap()
            .with_voice_chains(
                vec![VoiceChain::new(vec![incompatible], vec![], 0).unwrap()],
                vec![]
            )
            .is_err()
    );
}

#[test]
fn overlapping_voices_have_separate_history_and_choke_bounds_the_complete_chain() {
    let mut rt = Runtime::new(
        plan(vec![], vec![filter()], 8, Envelope::default(), 1),
        limits(),
    )
    .unwrap();
    support::without_heap(|| {
        let first = rt.trigger(input(1), 60, 1.).unwrap();
        let mut frame = [[0.; 2]];
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.25);
        let second = rt.trigger(input(2), 60, 0.5).unwrap();
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.625);
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.5);
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.125);
        rt.key_up(first, None).unwrap();
        rt.key_up(second, None).unwrap();
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));

        let note = rt.trigger(input(3), 60, 1.).unwrap();
        let family = rt.note_families(note).unwrap().next().unwrap();
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.25);
        rt.choke_family(family, 2).unwrap();
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.5);
        rt.choke_family(family, 100).unwrap();
        rt.render(&mut frame).unwrap();
        close(frame[0], 0.125);
        assert_eq!(
            (rt.voice_count(), rt.family_count()),
            (0, 0),
            "choke cannot be extended by DSP tail or a later longer fade"
        );
        rt.key_up(note, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
}

#[test]
fn batched_source_reads_preserve_filter_and_envelope_phase_at_midblock_release() {
    let source: Vec<_> = (0..193)
        .map(|n| [(n as f32 * 0.17).sin() * 0.25; 2])
        .collect();
    let prepared = || {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, source.clone().into_boxed_slice()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::new(4, 2, 3, 0.5, 5).unwrap(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_voice_chains(
            vec![VoiceChain::new(vec![filter()], vec![filter()], 17).unwrap()],
            vec![Some(0)],
        )
        .unwrap()
    };
    for release in [false, true] {
        let mut expected = [[0_f64; 2]; 224];
        let mut pre = [0.; 2];
        let mut post = [0.; 2];
        for (n, frame) in expected.iter_mut().enumerate() {
            let input = if release && n >= 96 {
                0.
            } else {
                source.get(n).map_or(0., |v| f64::from(v[0]))
            };
            let filtered = 0.25 * input + 0.5 * pre[0] + 0.25 * pre[1];
            pre = [input, pre[0]];
            let level = if release && n >= 91 {
                (0.5 * (1. - (n - 91) as f64 / 5.)).max(0.)
            } else if n < 4 {
                n as f64 / 4.
            } else if n < 6 {
                1.
            } else if n < 9 {
                1. - 0.5 * (n - 6) as f64 / 3.
            } else {
                0.5
            };
            let signal = filtered * f64::from(level as f32);
            *frame = [0.25 * signal + 0.5 * post[0] + 0.25 * post[1]; 2];
            post = [signal, post[0]];
        }
        let mut baseline = None;
        for block in [1, 7, 64, 129] {
            let mut rt = Runtime::new(prepared(), limits()).unwrap();
            let mut actual = [[0.; 2]; 224];
            support::without_heap(|| {
                let note = rt.trigger(input(1), 60, 1.).unwrap();
                if release {
                    rt.schedule_event(91, Event::KeyUp(note, None)).unwrap();
                }
                for chunk in actual.chunks_mut(block) {
                    rt.render(chunk).unwrap();
                }
                for (actual, expected) in actual.iter().zip(&expected) {
                    for channel in 0..2 {
                        assert!((f64::from(actual[channel]) - expected[channel]).abs() < 2e-8);
                    }
                }
                assert_eq!(rt.voice_count(), 0);
                if !release {
                    rt.key_up(note, None).unwrap();
                }
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
            if let Some(baseline) = baseline {
                assert_eq!(actual, baseline);
            } else {
                baseline = Some(actual);
            }
        }
    }
}

#[test]
fn dsp_state_capacity_and_chain_bindings_fail_before_runtime_publication() {
    assert!(matches!(
        Runtime::new(
            plan(vec![filter()], vec![], 0, Envelope::default(), 1),
            Limits {
                voices: usize::MAX,
                ..limits()
            }
        ),
        Err(sampler_core::Error::Capacity)
    ));
    for bindings in [vec![], vec![Some(1)], vec![None, None]] {
        let result = plan(vec![], vec![], 0, Envelope::default(), 1).with_voice_chains(
            vec![VoiceChain::new(vec![filter()], vec![], 0).unwrap()],
            bindings,
        );
        assert!(matches!(result, Err(sampler_core::Error::InvalidInput)));
    }
}

#[test]
fn stereo_matrices_preserve_both_inputs_order_filter_tails_and_voice_reuse() {
    let identity = [[1., 0.], [0., 1.]];
    let swap = [[0., 1.], [1., 0.]];
    let mid_side = [[0.5, 0.5], [0.5, -0.5]];
    let decode = [[1., 1.], [1., -1.]];
    let shear = [[1., 0.5], [0., 2.]];
    for (before, after, expected) in [
        (identity, identity, [1., -0.5]),
        (swap, identity, [-0.5, 1.]),
        ([[0.5; 2]; 2], identity, [0.25; 2]),
        (mid_side, decode, [1., -0.5]),
        (shear, swap, [-1., 0.75]),
        (swap, shear, [0., 2.]),
        ([[1., 0.25], [-0.5, 1.]], identity, [0.875, -1.]),
    ] {
        for block in [1, 7, 64] {
            let mut rt = Runtime::new(
                plan(
                    vec![Processor::StereoMatrix(before), filter()],
                    vec![Processor::StereoMatrix(after)],
                    3,
                    Envelope::default(),
                    1,
                ),
                limits(),
            )
            .unwrap();
            support::without_heap(|| {
                for id in [1, 2] {
                    let note = rt.trigger(input(id), 60, 1.).unwrap();
                    let mut audio = [[0.; 2]; 8];
                    for chunk in audio.chunks_mut(block) {
                        rt.render(chunk).unwrap();
                    }
                    for (index, frame) in audio.into_iter().enumerate() {
                        // This independently known filter has impulse [1/4, 1/2, 1/4].
                        let response = [0.25, 0.5, 0.25, 0., 0., 0., 0., 0.][index];
                        for channel in 0..2 {
                            assert!((frame[channel] - expected[channel] * response).abs() < 1e-7);
                        }
                    }
                    assert_eq!(rt.voice_count(), 0);
                    rt.key_up(note, None).unwrap();
                    rt.flush_ended(|_| true);
                    assert_eq!(rt.note_count(), 0);
                }
            });
        }
    }
    let mut rt = Runtime::new(
        plan(
            vec![
                Processor::StereoMatrix([[f64::MAX, -f64::MAX], [0., 1.]]),
                filter(),
            ],
            vec![],
            3,
            Envelope::default(),
            1,
        ),
        limits(),
    )
    .unwrap();
    support::without_heap(|| {
        let note = rt.trigger(input(1), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 8]);
        assert_eq!(rt.nonfinite_frames(), 1);
        rt.key_up(note, None).unwrap();
        rt.flush_ended(|_| true);
        assert_eq!(rt.note_count(), 0);
    });
    for index in 0..4 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut matrix = identity;
            matrix[index / 2][index % 2] = value;
            for pre in [true, false] {
                let stage = vec![Processor::StereoMatrix(matrix)];
                let (before, after) = if pre {
                    (stage, vec![])
                } else {
                    (vec![], stage)
                };
                assert!(VoiceChain::new(before, after, 0).is_err());
            }
        }
    }
}

#[test]
fn held_eq_and_rack_mix_render_and_recycle_without_heap_calls() {
    use sampler_core::{
        ControlDefinition, ControlDomain, ControlId, ControlRange, ControlValue, Parameter,
        PeakingEq,
    };
    let lane = |i| ControlRange {
        control: ControlId(i),
        low: 0.,
        high: 1.,
        ramp_frames: 0,
    };
    let p = plan(vec![], vec![], 0, Envelope::default(), 256)
        .with_controls(
            [0.2, 0.8, 0.25]
                .into_iter()
                .enumerate()
                .map(|(i, v)| ControlDefinition {
                    id: ControlId(i as u128),
                    domain: ControlDomain::Real { min: 0., max: 1. },
                    default: ControlValue::Real(v),
                })
                .collect(),
        )
        .unwrap()
        .with_voice_chains(
            vec![
                VoiceChain::new(
                    vec![
                        Processor::Mix {
                            count: 1,
                            dry: lane(0),
                            wet: lane(1),
                            bypass: lane(2),
                        },
                        Processor::PeakingEq(PeakingEq {
                            frequency: Parameter::Constant(0.6),
                            bandwidth: Parameter::Constant(0.4),
                            gain_db: Parameter::Constant(6.),
                        }),
                    ],
                    vec![],
                    0,
                )
                .unwrap(),
            ],
            vec![Some(0)],
        )
        .unwrap();
    let mut rt = Runtime::new(p, limits()).unwrap();
    for fragment in [1, 3, 7, 32, 64, 127, 256] {
        support::without_heap(|| {
            let note = rt.trigger(input(fragment as i32), 60, 1.).unwrap();
            let mut output = [[0.; 2]; 256];
            for chunk in output.chunks_mut(fragment) {
                rt.render(chunk).unwrap();
            }
            assert!(output.iter().flatten().all(|v| v.is_finite()));
            assert!(output[0][0].abs() > 0.1);
            rt.key_up(note, None).unwrap();
            rt.panic();
            rt.flush_ended(|_| true);
        });
    }
}
