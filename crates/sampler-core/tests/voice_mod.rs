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
        breakpoints: vec![],
        sources: vec![ModSource::Constant, ModSource::Velocity],
        routes: vec![ModRoute {
            scale: Some(ModScale {
                source: 1,
                shape: Some(0),
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
