use sampler_core::*;
mod support;

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
/// Frame k holds k / 1000 on the left and a constant 0.5 on the right.
fn plan(frames: usize, envelope: Envelope) -> Prepared {
    let pcm = (0..frames).map(|k| [k as f32 / 1000., 0.5]).collect();
    Prepared::new(
        48000,
        vec![Pcm::new(48000, pcm).unwrap()],
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
    .with_velocity_curves(vec![VelocityCurve::Constant])
    .unwrap()
}
fn program(sources: Vec<ModSource>, routes: Vec<ModRoute>) -> ModProgram {
    ModProgram {
        controls: vec![],
        breakpoints: vec![],
        sources,
        routes,
        shapes: vec![],
    }
}
fn modulated(plan: Prepared, program: ModProgram, start: u32) -> Prepared {
    plan.with_voice_modulation(vec![program], vec![Some(0)], vec![start])
        .unwrap()
}
fn render(rt: &mut Runtime, frames: usize, block: usize) -> Vec<Frame> {
    let mut audio = vec![[0.; 2]; frames];
    support::without_heap(|| {
        for chunk in audio.chunks_mut(block) {
            rt.render(chunk).unwrap();
        }
    });
    audio
}

#[test]
fn zero_multi_keeps_bipolar_volume_shape_and_voice_reuse_without_heap() {
    for block in [1, 7, 64, 137] {
        let source = ModSource::Lfo(Lfo {
            shape: LfoShape::Zero,
            rate: LfoRate::Hertz(4.),
            phase: 0.75,
            delay: 100,
            fade: 100,
            retrigger: true,
            shared: false,
        });
        let p = modulated(
            plan(1024, Envelope::default()),
            program(
                vec![source],
                vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
            ),
            0,
        );
        let mut rt = Runtime::new(p, limits()).unwrap();
        for id in [1, 2] {
            support::without_heap(|| {
                rt.trigger(input(id), 60, 1.).unwrap();
            });
            let audio = render(&mut rt, 512, block);
            for (i, frame) in audio.iter().enumerate() {
                assert_eq!(*frame, [i as f32 / 1000. * 0.5, 0.25]);
            }
            support::without_heap(|| {
                rt.note_off(input(id), None).unwrap();
                rt.render(&mut [[0.; 2]; 128]).unwrap();
            });
            assert_eq!(rt.voice_count(), 0);
        }
    }
}

#[test]
fn addressed_filters_do_not_leak_into_other_voices_without_heap() {
    let filtered = |hz| {
        VoiceChain::new(
            vec![],
            vec![Processor::StateVariable(StateVariableFilter {
                mode: SvfMode::LowPass,
                cutoff_hz: Parameter::Constant(hz),
                q: Parameter::Constant(0.7),
            })],
            0,
        )
        .unwrap()
    };
    let prepare = |addressed| {
        let region = |key| Region {
            sample: 0,
            key_low: key,
            key_high: key,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        };
        let base = Prepared::new(
            48000,
            vec![
                Pcm::new(
                    48000,
                    (0..512).map(|i| [(i as f32 * 0.17).sin(); 2]).collect(),
                )
                .unwrap(),
            ],
            vec![region(60), region(61)],
            2,
        )
        .unwrap()
        .with_velocity_curves(vec![VelocityCurve::Constant; 2])
        .unwrap();
        if addressed {
            base.with_voice_chains(vec![filtered(1000.)], vec![Some(0); 2])
                .unwrap()
                .with_voice_modulation(
                    vec![program(
                        vec![ModSource::Velocity],
                        vec![ModRoute::new(0, ModTarget::ProcessorCutoff(0), 12.)],
                    )],
                    vec![Some(0), None],
                    vec![0; 2],
                )
                .unwrap()
        } else {
            base.with_voice_chains(
                vec![filtered(2000.), filtered(1000.)],
                vec![Some(0), Some(1)],
            )
            .unwrap()
        }
    };
    for block in [1, 17, 64, 128] {
        let mut actual = Runtime::new(prepare(true), limits()).unwrap();
        let mut reference = Runtime::new(prepare(false), limits()).unwrap();
        let first = actual.trigger(input(1), 60, 1.).unwrap();
        let first_reference = reference.trigger(input(1), 60, 1.).unwrap();
        actual.trigger(input(2), 61, 1.).unwrap();
        reference.trigger(input(2), 61, 1.).unwrap();
        for after_release in [false, true] {
            if after_release {
                actual.release(first).unwrap();
                reference.release(first_reference).unwrap();
            }
            let actual = render(&mut actual, 128, block);
            let reference = render(&mut reference, 128, block);
            for (a, b) in actual.iter().zip(reference) {
                assert!(
                    (a[0] - b[0]).abs() < 1e-6,
                    "block {block}, release {after_release}: {a:?} != {b:?}"
                );
            }
        }
    }
}

#[test]
fn envelope_attenuation_ramps_exactly_under_any_partition_without_heap() {
    // A 64-frame linear attack read through Kontakt's attenuate law at depth 1.
    let attack = Envelope::new(64, 0, 0, 1., 0).unwrap();
    // Control points sit on the runtime's 64-frame grid, so any host block
    // partition reproduces the knee exactly.
    for block in [1, 7, 16, 61, 64, 100] {
        let mut rt = Runtime::new(
            modulated(
                plan(256, Envelope::default()),
                program(
                    vec![ModSource::Envelope(attack)],
                    vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
                ),
                0,
            ),
            limits(),
        )
        .unwrap();
        rt.trigger(input(1), 60, 1.).unwrap();
        let audio = render(&mut rt, 128, block);
        for (i, frame) in audio.iter().enumerate() {
            let level = ((i + 1) as f32 / 64.).min(1.);
            assert!(
                (frame[1] - 0.5 * level).abs() < 1e-6,
                "{block} {i} {frame:?}"
            );
        }
    }
}

#[test]
fn velocity_scales_gain_and_moves_sample_start() {
    let mut rt = Runtime::new(
        modulated(
            plan(256, Envelope::default()),
            program(
                vec![ModSource::Velocity],
                vec![
                    ModRoute::new(0, ModTarget::Attenuate, 1.),
                    ModRoute::new(0, ModTarget::SampleStart, 1.),
                ],
            ),
            100,
        ),
        limits(),
    )
    .unwrap();
    rt.trigger(input(1), 60, 0.5).unwrap();
    let audio = render(&mut rt, 4, 4);
    for (i, frame) in audio.iter().enumerate() {
        assert!(
            (frame[0] - 0.5 * (50 + i) as f32 / 1000.).abs() < 1e-6,
            "{audio:?}"
        );
        assert!((frame[1] - 0.25).abs() < 1e-6);
    }
}

#[test]
fn pitch_and_pan_routes_reach_the_voice() {
    // +12 semitones consumes a 128-frame source in 64 output frames.
    let mut rt = Runtime::new(
        modulated(
            plan(128, Envelope::default()),
            program(
                vec![ModSource::Constant],
                vec![
                    ModRoute::new(0, ModTarget::Pitch, 12.),
                    ModRoute::new(0, ModTarget::Pan, 1.),
                ],
            ),
            0,
        ),
        limits(),
    )
    .unwrap();
    rt.trigger(input(1), 60, 1.).unwrap();
    let audio = render(&mut rt, 70, 70);
    // Left is panned away; right settles after the resampler's onset ringing.
    assert!(audio[..64].iter().all(|f| f[0] == 0.), "{audio:?}");
    assert!(
        audio[16..48].iter().all(|f| (f[1] - 0.5).abs() < 1e-3),
        "{audio:?}"
    );
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn changing_and_held_controller_pitch_matches_expression_without_heap() {
    let build = |source, depth| {
        Runtime::new(
            modulated(
                plan(1024, Envelope::default()),
                program(
                    vec![source],
                    vec![ModRoute::new(0, ModTarget::Pitch, depth)],
                ),
                0,
            ),
            limits(),
        )
        .unwrap()
    };
    let mut actual = build(ModSource::Controller(1), 12.0);
    let mut reference = build(ModSource::Constant, 0.0);
    actual.trigger(input(1), 60, 1.0).unwrap();
    let note = reference.trigger(input(1), 60, 1.0).unwrap();
    let expression = reference.expression_id(note).unwrap();
    let performance = actual.performance(0).unwrap();
    // The first cell after a CC change holds the old/new pitch midpoint.
    for (cc, pitch) in [
        (0, 0.0),
        (u32::MAX, 6.0),
        (u32::MAX, 12.0),
        (0, 6.0),
        (0, 0.0),
    ] {
        let mut got = [[0.0; 2]; 64];
        let mut expected = got;
        support::without_heap(|| {
            actual.set_controller(performance, 1, cc).unwrap();
            reference
                .set_expression(
                    expression,
                    Expression {
                        pitch_semitones: pitch,
                        ..Default::default()
                    },
                )
                .unwrap();
            actual.render(&mut got).unwrap();
            reference.render(&mut expected).unwrap();
        });
        assert_eq!(
            got.map(|f| f.map(f32::to_bits)),
            expected.map(|f| f.map(f32::to_bits))
        );
    }
}

#[test]
fn modulated_cutoff_equals_the_static_filter_at_the_modulated_frequency() {
    let svf = |hz| {
        Processor::StateVariable(StateVariableFilter {
            mode: SvfMode::LowPass,
            cutoff_hz: Parameter::Constant(hz),
            q: Parameter::Constant(0.7),
        })
    };
    let chain = |plan: Prepared, hz| {
        plan.with_voice_chains(
            vec![VoiceChain::new(vec![], vec![svf(hz)], 0).unwrap()],
            vec![Some(0)],
        )
        .unwrap()
    };
    let mut reference =
        Runtime::new(chain(plan(256, Envelope::default()), 750.), limits()).unwrap();
    let mut swept = Runtime::new(
        modulated(
            chain(plan(256, Envelope::default()), 12000.),
            program(
                vec![ModSource::Constant],
                vec![ModRoute::new(0, ModTarget::Cutoff, -48.)],
            ),
            0,
        ),
        limits(),
    )
    .unwrap();
    reference.trigger(input(1), 60, 1.).unwrap();
    swept.trigger(input(1), 60, 1.).unwrap();
    let expected = render(&mut reference, 200, 64);
    let actual = render(&mut swept, 200, 13);
    for (a, b) in actual.iter().zip(&expected) {
        assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
    }
}

#[test]
fn modulation_envelopes_release_with_their_family() {
    // Sustain 1, release 128: the routed level falls linearly after key-up.
    let envelope = Envelope::new(0, 0, 0, 1., 128).unwrap();
    let mut rt = Runtime::new(
        modulated(
            plan(4096, Envelope::new(0, 0, 0, 1., 1000).unwrap()),
            program(
                vec![ModSource::Envelope(envelope)],
                vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
            ),
            0,
        ),
        limits(),
    )
    .unwrap();
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    render(&mut rt, 64, 64);
    rt.key_up(note, None).unwrap();
    let audio = render(&mut rt, 192, 64);
    for (i, frame) in audio[..128].iter().enumerate() {
        let level = 0.5 * (1. - (i + 1) as f32 / 1000.) * (1. - (i + 1) as f32 / 128.);
        assert!((frame[1] - level).abs() < 2e-3, "{i} {frame:?} {level}");
    }
    assert!(audio[128..].iter().all(|f| f[1] == 0.));
}

#[test]
fn rejects_out_of_range_programs() {
    let bad = |program: ModProgram| {
        plan(4, Envelope::default())
            .with_voice_modulation(vec![program], vec![Some(0)], vec![0])
            .is_err()
    };
    assert!(bad(program(
        vec![],
        vec![ModRoute::new(0, ModTarget::Pitch, 1.)]
    )));
    assert!(bad(program(
        vec![ModSource::Constant],
        vec![ModRoute::new(0, ModTarget::Pitch, f64::NAN)]
    )));
    assert!(bad(program(
        vec![ModSource::Lfo(Lfo {
            shape: LfoShape::Sine,
            rate: LfoRate::Hertz(0.),
            phase: 0.,
            delay: 0,
            fade: 0,
            retrigger: true,
            shared: false,
        })],
        vec![]
    )));
}

/// CPU cost per voice of modulation. Run in release with --ignored --nocapture.
#[test]
#[ignore]
fn measure_modulation_cost_per_voice() {
    const VOICES: usize = 64;
    const FRAMES: usize = 48000 * 4;
    let looped = |program: Option<ModProgram>, chain: bool| {
        let pcm: Box<[Frame]> = (0..48000)
            .map(|k| [((k as f32) * 0.031).sin() * 0.1; 2])
            .collect();
        let regions = (0..VOICES as u8)
            .map(|key| Region {
                sample: 0,
                key_low: key,
                key_high: key,
                root_key: Some(60),
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback {
                    loop_range: Some(Loop {
                        start: 0,
                        end: 48000,
                        mode: LoopMode::Continuous,
                        shape: LoopShape::Wrap,
                        passes: None,
                    }),
                    ..Playback::default()
                },
            })
            .collect();
        let mut plan =
            Prepared::new(48000, vec![Pcm::new(48000, pcm).unwrap()], regions, 1024).unwrap();
        if chain {
            plan = plan
                .with_voice_chains(
                    vec![
                        VoiceChain::new(
                            vec![],
                            vec![Processor::StateVariable(StateVariableFilter {
                                mode: SvfMode::LowPass,
                                cutoff_hz: Parameter::Constant(4000.),
                                q: Parameter::Constant(0.7),
                            })],
                            0,
                        )
                        .unwrap(),
                    ],
                    vec![Some(0); VOICES],
                )
                .unwrap();
        }
        if let Some(program) = program {
            plan = plan
                .with_voice_modulation(vec![program], vec![Some(0); VOICES], vec![0; VOICES])
                .unwrap();
        }
        let mut rt = Runtime::new(
            plan,
            Limits {
                notes: VOICES,
                voices: VOICES,
                families: VOICES,
                expressions: VOICES,
                commands: VOICES,
                ..limits()
            },
        )
        .unwrap();
        for key in 0..VOICES as u8 {
            let mut id = input(i32::from(key));
            id.key = key;
            rt.trigger_with_expression(
                id,
                key,
                1.,
                Expression {
                    timbre: 0x4000_0000,
                    ..Expression::default()
                },
            )
            .unwrap();
        }
        let mut out = vec![[0.; 2]; 256];
        let begin = std::time::Instant::now();
        for _ in 0..FRAMES / 256 {
            rt.render(&mut out).unwrap();
        }
        begin.elapsed().as_secs_f64() * 1e9 / (FRAMES * VOICES) as f64
    };
    let lfo = |shape, hz| {
        ModSource::Lfo(Lfo {
            shape,
            rate: LfoRate::Hertz(hz),
            phase: 0.,
            delay: 0,
            fade: 0,
            retrigger: true,
            shared: false,
        })
    };
    let mpe = || ModProgram {
        controls: vec![],
        breakpoints: vec![],
        sources: vec![ModSource::Pressure, ModSource::Timbre],
        routes: vec![
            ModRoute::new(0, ModTarget::Decibels, 6.),
            ModRoute {
                shape: Some(0),
                ..ModRoute::new(1, ModTarget::Tone, 60.)
            },
        ],
        shapes: vec![vec![(0., -1.), (0.5, 0.), (1., 0.)]],
    };
    let full = ModProgram {
        controls: vec![],
        breakpoints: vec![],
        sources: vec![
            lfo(LfoShape::Sine, 5.),
            ModSource::Envelope(Envelope::new(4800, 0, 9600, 0.5, 4800).unwrap()),
            lfo(LfoShape::Triangle, 0.3),
        ],
        routes: vec![
            ModRoute::new(0, ModTarget::Pitch, 0.3),
            ModRoute::new(1, ModTarget::Attenuate, 1.),
            ModRoute::new(2, ModTarget::Cutoff, 24.),
            ModRoute::new(0, ModTarget::Pan, 0.2),
        ],
        shapes: vec![],
    };
    let base = looped(None, false);
    let rest_mpe = {
        let mut p = mpe();
        p.routes.truncate(1);
        p.sources.truncate(1);
        p.shapes.clear();
        looped(Some(p), false)
    };
    let tone = looped(Some(mpe()), false);
    let chain = looped(None, true);
    let chain_full = looped(Some(full), true);
    println!(
        "ns per voice-frame: plain {base:.2}, +pressure route {rest_mpe:.2}, +closed tone {tone:.2}"
    );
    println!(
        "ns per voice-frame: svf chain {chain:.2}, +lfo pitch/pan, env gain, lfo cutoff {chain_full:.2}"
    );
}

#[test]
fn a_second_source_scales_route_depth() {
    // Constant +6 dB, depth scaled by velocity (0.5) through a shape doubling it
    // past 0.25: multiplier 0.5 + (0.5 - 0.25) = 0.75 => +4.5 dB.
    let program = ModProgram {
        controls: vec![],
        breakpoints: vec![],
        sources: vec![ModSource::Constant, ModSource::Velocity],
        routes: vec![ModRoute {
            scale: Some(ModScale {
                source: 1,
                shape: Some(0),
                law: ModScaleLaw::Multiply,
            }),
            ..ModRoute::new(0, ModTarget::Decibels, 6.)
        }],
        shapes: vec![vec![(0., 0.), (0.25, 0.25), (1., 1.75)]],
    };
    let mut rt = Runtime::new(
        modulated(plan(64, Envelope::default()), program, 0),
        limits(),
    )
    .unwrap();
    rt.trigger(input(1), 60, 0.5).unwrap();
    let audio = render(&mut rt, 8, 8);
    let expected = 0.5 * 10f32.powf(4.5 / 20.);
    assert!(
        audio.iter().all(|f| (f[1] - expected).abs() < 1e-5),
        "{audio:?}"
    );
}

#[test]
fn id25_intensity_changes_each_outgoing_depth_in_render_without_heap() {
    for flags in [0x10, 0x14] {
        for base in [0., 0.4, 0.9] {
            for velocity in [0.25, 0.75] {
                for bipolar in [false, true] {
                    let second = if bipolar {
                        ModSource::Lfo(Lfo { shape: LfoShape::Zero,
                            rate: LfoRate::Hertz(1.), phase: 0., delay: 0, fade: 0,
                            retrigger: true, shared: false })
                    } else { ModSource::Velocity };
                    let p = program(vec![ModSource::Constant, second], vec![ModRoute {
                        invert: true,
                        scale: Some(ModScale { source: 1, shape: None,
                            law: ModScaleLaw::KontaktIntensity { depth: 0.5, flags, unit: 1. } }),
                        ..ModRoute::new(0, ModTarget::Attenuate, base)
                    }]);
                    let mut rt = Runtime::new(modulated(plan(64, Envelope::default()), p, 0),
                        limits()).unwrap();
                    support::without_heap(|| { rt.trigger(input(1), 60, velocity).unwrap(); });
                    let audio = render(&mut rt, 8, 8);
                    let source = if bipolar { 0.5 } else { velocity };
                    let depth = if flags & 4 == 0 {
                        base * (1. - (1. - source) * 0.5)
                    } else { 1. - (1. - base) * (1. - source * 0.5) };
                    let expected = (0.5 * (1. - depth)) as f32;
                    assert!(audio.iter().all(|f| (f[1] - expected).abs() < 1e-7),
                        "flags={flags} base={base} bipolar={bipolar} {audio:?}");
                }
            }
        }
    }
}

#[test]
fn id25_unverified_lag_init_shape_and_live_paths_are_rejected() {
    let scale = ModScale { source: 1, shape: None,
        law: ModScaleLaw::KontaktIntensity { depth: 0.5, flags: 0x10, unit: 1. } };
    let route = ModRoute { scale: Some(scale),
        ..ModRoute::new(0, ModTarget::Attenuate, 0.4) };
    let rejected = |source, route| {
        plan(64, Envelope::default()).with_voice_modulation(
            vec![program(vec![ModSource::Constant, source], vec![route])],
            vec![Some(0)], vec![0]).is_err()
    };
    assert!(rejected(ModSource::Velocity, ModRoute { lag: 1, ..route }));
    assert!(rejected(ModSource::Velocity, ModRoute { target: ModTarget::SampleStart, ..route }));
    for flags in [0, 4, 0x12, 0x18, 0xff] {
        assert!(rejected(ModSource::Velocity, ModRoute {
            scale: Some(ModScale { law: ModScaleLaw::KontaktIntensity { depth: 0.5, flags, unit: 1. },
                ..scale }), ..route }));
    }
    for depth in [f64::NAN, f64::INFINITY, -0.25, 1.25] {
        assert!(rejected(ModSource::Velocity, ModRoute {
            scale: Some(ModScale { law: ModScaleLaw::KontaktIntensity { depth, flags: 0x10, unit: 1. },
                ..scale }), ..route }));
    }
    for unit in [0., f64::NAN, f64::INFINITY, 1e-310] {
        assert!(rejected(ModSource::Velocity, ModRoute {
            scale: Some(ModScale { law: ModScaleLaw::KontaktIntensity {
                depth: 0.5, flags: 0x10, unit }, ..scale }), ..route }));
    }
    for source in [ModSource::Controller(1), ModSource::Pressure, ModSource::Timbre,
        ModSource::PitchBend, ModSource::Script(0)] {
        assert!(rejected(source, route));
    }
    let mut p = program(vec![ModSource::Constant, ModSource::Velocity], vec![ModRoute {
        scale: Some(ModScale { shape: Some(0), ..scale }), ..route }]);
    p.shapes.push(vec![(0., 0.), (1., 1.)]);
    assert!(plan(64, Envelope::default()).with_voice_modulation(
        vec![p], vec![Some(0)], vec![0]).is_err());
}

#[test]
fn id25_additive_depth_converts_normalized_intensity_to_outgoing_units() {
    for flags in [0x10, 0x14] {
        for unit in [12., -12., 20., -20.] {
            for base in [0., 0.4, 1.] {
                let p = program(vec![ModSource::Constant, ModSource::Velocity], vec![ModRoute {
                    scale: Some(ModScale { source: 1, shape: None,
                        law: ModScaleLaw::KontaktIntensity { depth: 0.5, flags, unit } }),
                    ..ModRoute::new(0, ModTarget::Decibels, base * unit)
                }]);
                let mut rt = Runtime::new(modulated(plan(64, Envelope::default()), p, 0),
                    limits()).unwrap();
                support::without_heap(|| { rt.trigger(input(1), 60, 0.25).unwrap(); });
                let audio = render(&mut rt, 8, 8);
                let adjusted = if flags & 4 == 0 { base * 0.625 }
                    else { 1. - (1. - base) * 0.875 };
                let expected = (0.5 * 10f64.powf(adjusted * unit / 20.)) as f32;
                assert!(audio.iter().all(|f| (f[1] - expected).abs() < 2e-6),
                    "flags={flags} base={base} unit={unit} {audio:?}");
            }
        }
    }
}

#[test]
fn id25_preserves_independent_bases_for_two_outgoing_targets() {
    let routes = [(0., 20.), (0.4, 12.)].map(|(base, unit)| ModRoute {
        scale: Some(ModScale { source: 1, shape: None,
            law: ModScaleLaw::KontaktIntensity { depth: 0.5, flags: 0x14, unit } }),
        ..ModRoute::new(0, ModTarget::Decibels, base * unit)
    });
    let p = program(vec![ModSource::Constant, ModSource::Velocity], routes.into());
    let mut rt = Runtime::new(modulated(plan(64, Envelope::default()), p, 0),
        limits()).unwrap();
    support::without_heap(|| { rt.trigger(input(1), 60, 0.25).unwrap(); });
    let audio = render(&mut rt, 8, 8);
    // At src=.25/depth=.5, each saved target gets its own additive result:
    // 0 -> .125 (20 units), .4 -> .475 (12 units); neither shares a gain stage.
    let expected = (0.5 * 10f64.powf((0.125 * 20. + 0.475 * 12.) / 20.)) as f32;
    assert!(audio.iter().all(|f| (f[1] - expected).abs() < 2e-6), "{audio:?}");
}

#[test]
fn breakpoint_envelope_glides_holds_at_sustain_and_releases() {
    use sampler_core::{Breakpoint, Breakpoints, EnvelopeCurve};
    let point = |frames, level| Breakpoint {
        frames,
        level,
        curve: EnvelopeCurve::default(),
    };
    // 0 -> 1 over 64 frames, hold at 1, then down to 0 over 128 on release.
    let mut program = program(
        vec![ModSource::Breakpoints(0)],
        vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
    );
    program.breakpoints = vec![Breakpoints {
        points: vec![point(64, 1.), point(128, 0.)],
        sustain: Some(0),
    }];
    let mut rt = Runtime::new(
        modulated(
            plan(
                4096,
                // The gate holds through the breakpoint release (a step release).
                Envelope::new(0, 0, 0, 1., 512).unwrap().with_curves(
                    EnvelopeCurve::default(),
                    EnvelopeCurve::default(),
                    EnvelopeCurve::step(),
                ),
            ),
            program,
            0,
        ),
        limits(),
    )
    .unwrap();
    {
        rt.trigger(input(1), 60, 1.).unwrap();
        let audio = render(&mut rt, 256, 61);
        for (i, frame) in audio[..64].iter().enumerate() {
            assert!((frame[1] - 0.5 * (i + 1) as f32 / 64.).abs() < 1e-6, "{i}");
        }
        assert!(audio[64..].iter().all(|f| (f[1] - 0.5).abs() < 1e-6));
        rt.note_off(input(1), None).unwrap();
        let audio = render(&mut rt, 192, 7);
        for (i, frame) in audio[..128].iter().enumerate() {
            assert!(
                (frame[1] - 0.5 * (1. - (i + 1) as f32 / 128.)).abs() < 1e-6,
                "{i}"
            );
        }
        assert!(audio[128..].iter().all(|f| f[1].abs() < 1e-6));
    }
}

#[test]
fn release_counter_counts_down_while_held_and_freezes_at_key_up() {
    let program = program(
        vec![ModSource::ReleaseCounter { frames: 256 }],
        vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
    );
    let gate = Envelope::new(0, 0, 0, 1., 512).unwrap().with_curves(
        EnvelopeCurve::default(),
        EnvelopeCurve::default(),
        EnvelopeCurve::step(),
    );
    let mut rt = Runtime::new(modulated(plan(4096, gate), program, 0), limits()).unwrap();
    rt.trigger(input(1), 60, 1.).unwrap();
    let held = render(&mut rt, 128, 64);
    // Counting down: louder at the start than near key-up.
    assert!(held[0][1] > held[127][1] + 0.1);
    rt.note_off(input(1), None).unwrap();
    // Frozen at 1 − 128/256 once the ramp into it completes.
    let after = render(&mut rt, 256, 64);
    assert!(
        after[64..].iter().all(|f| (f[1] - 0.25).abs() < 1e-6),
        "{:?}",
        after[64]
    );
}

#[test]
fn pitch_bend_drives_non_pitch_routes_from_its_raw_position() {
    // Bipolar: attenuate reads (bend + 1) / 2, so rest is half gain.
    let program = program(
        vec![ModSource::PitchBend],
        vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
    );
    let mut rt = Runtime::new(
        modulated(plan(4096, Envelope::default()), program, 0),
        limits(),
    )
    .unwrap();
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    let rest = render(&mut rt, 128, 64);
    assert!((rest[127][1] - 0.25).abs() < 1e-6, "{:?}", rest[127]);
    let id = rt.expression_id(note).unwrap();
    rt.set_expression(
        id,
        Expression {
            bend: 1.,
            ..Expression::default()
        },
    )
    .unwrap();
    let full = render(&mut rt, 192, 64);
    assert!((full[191][1] - 0.5).abs() < 1e-6, "{:?}", full[191]);
}

#[test]
fn release_counter_reset_retargets_live_modulation_and_pedals_preserve_release_age() {
    let program = program(
        vec![ModSource::ReleaseCounter { frames: 256 }],
        vec![ModRoute::new(0, ModTarget::Attenuate, 1.)],
    );
    let gate = Envelope::new(0, 0, 0, 1., 512).unwrap().with_curves(
        EnvelopeCurve::default(),
        EnvelopeCurve::default(),
        EnvelopeCurve::step(),
    );
    let mut rt = Runtime::new(
        modulated(plan(4096, gate), program, 0),
        Limits {
            channels: 1,
            ..limits()
        },
    )
    .unwrap();
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    render(&mut rt, 128, 64);
    assert_eq!(rt.release_counter_frames(note).unwrap(), 128);
    rt.reset_release_counter(note).unwrap();
    assert_eq!(rt.release_counter_frames(note).unwrap(), 0);
    let channel = rt.register_channel(input(1).channel_address()).unwrap();
    rt.sustain(channel, true).unwrap();
    render(&mut rt, 64, 16);
    rt.note_off(input(1), None).unwrap();
    assert_eq!(rt.release_counter_frames(note).unwrap(), 64);
    let tail = render(&mut rt, 128, 16);
    assert!(tail[64..].iter().all(|f| (f[1] - 0.375).abs() < 1e-6));
    assert_eq!(rt.release_counter_frames(note).unwrap(), 64);
    rt.sustain(channel, false).unwrap();
    assert_eq!(rt.release_context(note).unwrap().gate.unwrap().at, 320);
    assert_eq!(rt.release_context(note).unwrap().key.unwrap().at, 192);
}

#[test]
fn native_lanes_with_script_and_gain_pitch_ramps_match_forced_scalar_without_heap() {
    // 97 covers an eight-voice remainder and the three-thread dispatch threshold.
    const N: usize = 97;
    let prepared = |scalar, streamed| {
        let chain = || {
            VoiceChain::new(
                vec![
                    Processor::Gainer {
                        dry: 0.125,
                        gain: Parameter::Constant(0.8),
                    },
                    Processor::StereoModeller(StereoSettings {
                        width: Parameter::Constant(0.7),
                        pan: Parameter::Constant(-0.2),
                        pseudo: false,
                    }),
                ],
                vec![Processor::StateVariable(StateVariableFilter {
                    mode: SvfMode::LowPass,
                    cutoff_hz: Parameter::Constant(6000.),
                    q: Parameter::Constant(0.7),
                })],
                128,
            )
            .unwrap()
        };
        let regions = (0..N)
            .map(|i| Region {
                sample: 0,
                key_low: 16 + i as u8,
                key_high: 16 + i as u8,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 0.01,
                envelope: Envelope::new(4, 2, 12, 0.6, 17).unwrap(),
                playback: Playback {
                    start: PAGE_FRAMES - 128,
                    ..Playback::default()
                },
            })
            .collect();
        let pcm: Box<[Frame]> = (0..PAGE_FRAMES * 2)
            .map(|i| [(i as f32 * 0.17).sin(), (i as f32 * 0.11).cos()])
            .collect();
        let asset = if streamed {
            Pcm::streamed(48000, pcm.len()).unwrap()
        } else {
            Pcm::new(48000, pcm.clone()).unwrap()
        };
        let storage = streamed.then(|| {
            let (mut cache, mut worker) = StreamCache::new(N * 2).unwrap();
            for page in 0..2 {
                assert_eq!(cache.request(&asset, page, 0), Ok(PageStatus::Pending));
                let mut job = worker.next_job().unwrap();
                let range = job.range();
                job.frames_mut().copy_from_slice(&pcm[range]);
                worker.complete(job, Ok(())).unwrap();
                assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
            }
            (cache, worker)
        });
        let plan = Prepared::new(48000, vec![asset], regions, N)
            .unwrap()
            .with_voice_chains(
                vec![chain(), chain()],
                (0..N)
                    .map(|i| Some(if scalar { i % 2 } else { 0 }))
                    .collect(),
            )
            .unwrap()
            .with_voice_modulation(
                vec![program(
                    vec![
                        ModSource::Velocity,
                        ModSource::Controller(1),
                        ModSource::Lfo(Lfo {
                            shape: LfoShape::Sine,
                            rate: LfoRate::Hertz(5.),
                            phase: 0.,
                            delay: 0,
                            fade: 0,
                            retrigger: true,
                            shared: false,
                        }),
                    ],
                    vec![
                        ModRoute::new(0, ModTarget::Decibels, -6.),
                        ModRoute::new(1, ModTarget::Pan, 0.5),
                        ModRoute::new(2, ModTarget::Pitch, 3.),
                        ModRoute::new(2, ModTarget::Decibels, 2.),
                    ],
                )],
                vec![Some(0); N],
                vec![0; N],
            )
            .unwrap();
        (plan, storage)
    };
    let capacity = Limits {
        notes: N,
        voices: N,
        families: N,
        expressions: N,
        commands: 16,
        ..limits()
    };
    for streamed in [false, true] {
        for threads in [1, 3] {
            for block in [1, 17, 64, 128] {
                let runtime = |scalar| {
                    let (plan, storage) = prepared(scalar, streamed);
                    let mut rt = Runtime::new(plan, capacity)
                        .unwrap()
                        .with_threads(Threads::Fixed(threads));
                    let worker = if let Some((cache, worker)) = storage {
                        rt = rt.with_stream_cache(cache);
                        Some(worker)
                    } else {
                        None
                    };
                    (rt, worker)
                };
                let (mut actual, _actual_worker) = runtime(false);
                let (mut reference, _reference_worker) = runtime(true);
                let mut notes = [Vec::with_capacity(N), Vec::with_capacity(N)];
                for (side, rt) in [&mut actual, &mut reference].into_iter().enumerate() {
                    support::without_heap(|| {
                        for i in 0..N {
                            let key = 16 + i as u8;
                            let note = rt
                                .trigger(
                                    Input {
                                        key,
                                        ..input(i as i32)
                                    },
                                    key,
                                    0.3 + (i % 7) as f64 * 0.1,
                                )
                                .unwrap();
                            rt.set_note_param(note, ModTarget::Decibels, -((i % 8) as f64), false)
                                .unwrap();
                            notes[side].push(note);
                        }
                    });
                }
                for (phase, frames) in [96, 128, 128, 192].into_iter().enumerate() {
                    for (side, rt) in [&mut actual, &mut reference].into_iter().enumerate() {
                        support::without_heap(|| match phase {
                            1 => {
                                let performance = rt.performance(0).unwrap();
                                rt.set_controller(performance, 1, u32::MAX).unwrap();
                                rt.set_note_param(notes[side][5], ModTarget::Pitch, 12., false)
                                    .unwrap();
                                rt.set_note_param(notes[side][9], ModTarget::Pan, -0.4, false)
                                    .unwrap();
                            }
                            2 => rt.fade_note(notes[side][0], None, 0., 64, true).unwrap(),
                            3 => {
                                for &note in &notes[side][1..] {
                                    rt.release(note).unwrap();
                                }
                            }
                            _ => {}
                        });
                    }
                    let a = render(&mut actual, frames, block);
                    let b = render(&mut reference, frames, block);
                    assert_eq!(
                        a, b,
                        "streamed {streamed}, threads {threads}, block {block}, phase {phase}"
                    );
                    assert_eq!(actual.voice_count(), reference.voice_count());
                    assert_eq!(actual.stream_underruns(), 0);
                    assert_eq!(reference.stream_underruns(), 0);
                }
                if threads > 1 {
                    assert!(actual.parallel_blocks() > 0);
                }
            }
        }
    }
}
