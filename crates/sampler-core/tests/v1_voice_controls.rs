//! Literal pinned-v1 controls checked through the exported prepared-control API.
#![allow(dead_code)]
use sampler_core::Error;
#[path = "support/v1_voice_controls_oracle.rs"]
mod oracle;
mod support;
use sampler_core::v1_voice_controls;
use std::sync::Arc;
use v1_voice_controls::*;

fn inputs(cc: &[u8; 128], stamp: u32) -> Inputs<'_> {
    Inputs {
        cc,
        cc74: Some(79),
        bend: -0.25,
        pressure: 33,
        note: 60,
        velocity: 100,
        counter: 0.3,
        stamp,
        bend_pitch: Some(0.125),
    }
}
fn ahdsr(curve: f32) -> Ahdsr {
    Ahdsr {
        attack: 0.12501293,
        hold: 0.01,
        decay: 0.1,
        sustain: 0.3,
        release: 0.2500013,
        curve,
        ahd_only: false,
    }
}
fn bits(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "frame {i}");
    }
}
fn render(
    state: &mut ControlState,
    plan: &ControlPlan,
    n: usize,
    amp: &mut [f32; 128],
    flex: Option<&mut [f32; 128]>,
    vol: Option<&mut [f32; 128]>,
) -> (u64, u64) {
    let mut positions = [0; 128];
    state
        .render(
            plan,
            120.,
            1.,
            &mut amp[..n],
            flex.map(|x| &mut x[..n]),
            vol.map(|x| &mut x[..n]),
            &mut positions[..n],
        )
        .unwrap()
}

#[test]
fn native_primary_has_pinned_v1_f32_checkpoints() {
    for (curve, expected) in [
        (-1., [0, 0x36368000, 0x3cc8ae68, 0x3f72cb62]),
        (-0.5, [0, 0x3a28e540, 0x3e83a4af, 0x3f7b41e2]),
        (0., [0, 0x3babe000, 0x3f1f0840, 0x3f7eaa80]),
        (0.5, [0, 0x3c97cbc0, 0x3f69029f, 0x3f7fd5c7]),
        (1., [0, 0x3d534a80, 0x3f7f7dc7, 0x3f7fffd2]),
    ] {
        let amplitude = Ahdsr {
            hold: 0.,
            decay: 0.,
            sustain: 1.,
            ..ahdsr(curve)
        };
        let plan = ControlPlan::prepare(
            ControlDescription {
                amplitude,
                native_amplitude: true,
                ..Default::default()
            },
            48000.,
        )
        .unwrap();
        let mut probe = ControlState::new(&plan);
        let mut amp = [0.; 128];
        render(&mut probe, &plan, 64, &mut amp, None, None);
        assert_eq!(probe.amplitude_control_point().to_bits(), expected[1]);
        assert_eq!(probe.amplitude_control_point().to_bits(), expected[1]);
        render(&mut probe, &plan, 1, &mut amp, None, None);
        assert_eq!(amp[0].to_bits(), expected[1]);
        let mut state = ControlState::new(&plan);
        let mut samples = [0.; 6016];
        for out in samples.chunks_mut(128) {
            let mut positions = [0; 128];
            state
                .render(
                    &plan,
                    120.,
                    1.,
                    out,
                    None,
                    None,
                    &mut positions[..out.len()],
                )
                .unwrap();
        }
        for (frame, expected) in [0, 64, 3776, 5984].into_iter().zip(expected) {
            assert_eq!(
                samples[frame].to_bits(),
                expected,
                "curve={curve} frame={frame}"
            );
        }
        assert!(
            state.shape(128).is_none(),
            "native amplitude cannot enter the ordinary decay lane"
        );
    }
}

#[test]
fn ordinary_native_flex_and_skip_match_literal_v1_at_event_fragments() {
    let cc = [0; 128];
    for native in [false, true] {
        for curve in [-1., -0.6, 0., 0.3, 1.] {
            for flex in [
                None,
                Some(Flex {
                    points: vec![
                        FlexPoint {
                            seconds: 0.013,
                            level: 1.,
                            curve: 0.7,
                        },
                        FlexPoint {
                            seconds: 0.05,
                            level: 0.3,
                            curve: -0.4,
                        },
                        FlexPoint {
                            seconds: 0.2,
                            level: 0.,
                            curve: 0.2,
                        },
                    ]
                    .into_boxed_slice(),
                    sustain: 1,
                }),
            ] {
                let plan = ControlPlan::prepare(
                    ControlDescription {
                        amplitude: ahdsr(curve),
                        native_amplitude: native,
                        flex,
                        ..Default::default()
                    },
                    48000.,
                )
                .unwrap();
                let mut state = ControlState::new(&plan);
                state.reset(&plan, &inputs(&cc, 7), 17, 48000.).unwrap();
                let mut skipped = state.clone();
                let mut reference = oracle::EnvelopeOracle::new(
                    &plan.description().amplitude,
                    native,
                    plan.description().flex.is_some(),
                    48000.,
                );
                let mut expected_skip = oracle::EnvelopeOracle::new(
                    &plan.description().amplitude,
                    native,
                    plan.description().flex.is_some(),
                    48000.,
                );
                for block in 0..600 {
                    if block == 60 {
                        state.release(&plan);
                        state.release(&plan);
                        skipped.release(&plan);
                        skipped.release(&plan);
                        reference.release(plan.description().flex.as_ref());
                        expected_skip.release(plan.description().flex.as_ref());
                    }
                    let n = [0, 1, 7, 8, 9, 17, 31, 32, 33, 77, 128][block % 11];
                    let (mut amp, mut amp_ref, mut flex, mut flex_ref) =
                        ([0.; 128], [0.; 128], [0.; 128], [0.; 128]);
                    let has_flex = plan.description().flex.is_some();
                    render(
                        &mut state,
                        &plan,
                        n,
                        &mut amp,
                        has_flex.then_some(&mut flex),
                        None,
                    );
                    reference.render(
                        &mut amp_ref[..n],
                        has_flex.then_some(&mut flex_ref[..n]),
                        plan.description().flex.as_ref(),
                        48000.,
                    );
                    skipped.skip(&plan, 120., 1., n).unwrap();
                    expected_skip.skip(n, plan.description().flex.as_ref(), 48000.);
                    bits(&amp[..n], &amp_ref[..n]);
                    if has_flex {
                        bits(&flex[..n], &flex_ref[..n]);
                    }
                    assert_eq!(state.level().to_bits(), reference.level().to_bits());
                    assert_eq!(skipped.level().to_bits(), expected_skip.level().to_bits());
                    assert_eq!(state.level().to_bits(), skipped.level().to_bits());
                    assert_eq!(
                        (state.phase(), state.done()),
                        (reference.phase(), reference.done())
                    );
                }
            }
        }
    }
}
fn source(phase: f32, fade: f32) -> PitchLfo {
    PitchLfo {
        start_phase: phase,
        slot: 7,
        count: 16.,
        note_value: 1. / 24.,
        sine: 1.,
        fade_ms: fade,
        depth: 0.25,
        targets: vec![(2, 0.25)],
        bypassed: false,
    }
}
#[test]
fn lfo_preview_actual_fade_shared_volume_bypass_and_resume_match_literal_v1() {
    for phase in [0., 0.25, 0.4990234375, 0.5, 0.5009765625, 0.75, 1.] {
        for (pitch, volume) in [(true, false), (false, true), (true, true)] {
            let source = source(phase, if volume { 0. } else { 13. });
            let desc = ControlDescription {
                pitch_lfos: if pitch {
                    vec![source.clone()].into_boxed_slice()
                } else {
                    Box::new([])
                },
                volume_lfos: if volume {
                    vec![VolumeLfo {
                        source: PitchLfo {
                            depth: 0.,
                            targets: Vec::new(),
                            ..source.clone()
                        },
                        target: 4,
                        intensity: 1.,
                        negative: false,
                        lag_ms: 15,
                    }]
                    .into_boxed_slice()
                } else {
                    Box::new([])
                },
                ..Default::default()
            };
            let plan = ControlPlan::prepare(desc.clone(), 48000.).unwrap();
            let mut bypass = desc;
            for p in &mut bypass.pitch_lfos {
                p.bypassed = true;
            }
            for v in &mut bypass.volume_lfos {
                v.source.bypassed = true;
            }
            let bypass = ControlPlan::prepare(bypass, 48000.).unwrap();
            let mut state = ControlState::new(&plan);
            let mut reference = oracle::LfoOracle::default();
            for block in 0..150 {
                let plan = if (30..45).contains(&block) {
                    &bypass
                } else {
                    &plan
                };
                let d = plan.description();
                let n = [0, 1, 7, 17, 31, 32, 33, 77, 128][block % 9];
                let (
                    mut preview,
                    mut expected_preview,
                    mut positions,
                    mut expected,
                    mut volumes,
                    mut expected_volume,
                    mut amp,
                ) = (
                    [0; 128], [0; 128], [0; 128], [0; 128], [0.; 128], [0.; 128], [0.; 128],
                );
                let mut copied = reference;
                let got = state
                    .preview_pitch(plan, 120., 1.03125, &mut preview[..n])
                    .unwrap();
                let want = copied.positions(
                    &d.pitch_lfos,
                    &[],
                    48000.,
                    120.,
                    1.03125,
                    &mut expected_preview[..n],
                    None,
                );
                assert_eq!((got, &preview[..n]), (want, &expected_preview[..n]));
                let got = state
                    .render(
                        plan,
                        120.,
                        1.03125,
                        &mut amp[..n],
                        None,
                        volume.then_some(&mut volumes[..n]),
                        &mut positions[..n],
                    )
                    .unwrap();
                let want = reference.positions(
                    &d.pitch_lfos,
                    &d.volume_lfos,
                    48000.,
                    120.,
                    1.03125,
                    &mut expected[..n],
                    volume.then_some(&mut expected_volume[..n]),
                );
                assert_eq!((got, &positions[..n]), (want, &expected[..n]));
                if volume {
                    bits(&volumes[..n], &expected_volume[..n]);
                }
            }
        }
    }
}
fn mods() -> ModTable {
    let curve = Arc::new(std::array::from_fn(|i| (i as f32 / 127.).powi(2)));
    ModTable::new(
        vec![
            Mod {
                route: Some((Source::Velocity, Target::Volume)),
                intensity: -0.7,
                lag: 0.,
                curve: Some(curve.clone()),
            },
            Mod {
                route: Some((Source::Cc(1), Target::Volume)),
                intensity: 0.8,
                lag: 0.05,
                curve: Some(curve.clone()),
            },
            Mod {
                route: Some((Source::Bend, Target::Pitch)),
                intensity: -0.3,
                lag: 0.003,
                curve: None,
            },
            Mod {
                route: Some((Source::Cc(74), Target::Pitch)),
                intensity: 0.2,
                lag: 0.,
                curve: Some(curve),
            },
            Mod {
                route: Some((Source::Pressure, Target::Volume)),
                intensity: 0.15,
                lag: 0.02,
                curve: None,
            },
            Mod {
                route: Some((Source::Constant, Target::Pitch)),
                intensity: 0.1,
                lag: 0.,
                curve: None,
            },
            Mod {
                route: Some((Source::Counter, Target::Pitch)),
                intensity: -0.2,
                lag: 0.,
                curve: None,
            },
        ]
        .into_boxed_slice(),
    )
    .unwrap()
}
#[test]
fn external_modulation_settled_stamp_and_pitch_envelopes_match_literal_v1() {
    let pitch_env = PitchEnvelope {
        env: Ahdsr {
            attack: 0.003,
            decay: 0.007,
            release: 0.009,
            ..ahdsr(0.8)
        },
        bypass: false,
        index: 9,
        targets: vec![(
            4,
            -1.,
            Mod {
                route: Some((Source::Constant, Target::Pitch)),
                intensity: 0.3,
                lag: 0.,
                curve: None,
            },
        )]
        .into_boxed_slice(),
    };
    let plan = ControlPlan::prepare(
        ControlDescription {
            mods: mods(),
            pitch_envelopes: vec![pitch_env].into_boxed_slice(),
            ..Default::default()
        },
        48000.,
    )
    .unwrap();
    let mut cc = [0; 128];
    let mut stamp = 0;
    let input = inputs(&cc, stamp);
    let mut state = ControlState::new(&plan);
    state.reset(&plan, &input, 137, 48000.).unwrap();
    let mut reference = oracle::ModOracle::new(plan.description().mods.assignments(), &input);
    let mut pitch = oracle::PitchOracle::new(&plan.description().pitch_envelopes, 48000.);
    for block in 0..600 {
        if block % 23 == 0 {
            cc[1] = ((block * 13) % 128) as u8;
            stamp += 1;
        }
        if block == 200 {
            state.release(&plan);
            pitch.release();
        }
        let input = inputs(&cc, stamp);
        let n = [1, 7, 17, 32, 77, 128][block % 6];
        let got = state.plan_controls(&plan, &input, n);
        let (gain, semitones, settled) = reference.modulate(&input, n, 48000.);
        let semitones = semitones + pitch.pitch(&plan.description().pitch_envelopes, n, 48000.);
        assert_eq!(
            (got.0.to_bits(), got.1.to_bits(), got.2),
            (gain.to_bits(), semitones.to_bits(), settled),
            "block{block}"
        );
    }
}
#[test]
fn reset_release_render_skip_and_reuse_do_no_heap_work() {
    let src = source(0.25, 0.);
    let plan = ControlPlan::prepare(
        ControlDescription {
            amplitude: ahdsr(0.5),
            native_amplitude: true,
            pitch_lfos: vec![src.clone()].into_boxed_slice(),
            volume_lfos: vec![VolumeLfo {
                source: src,
                target: 4,
                intensity: 0.7,
                negative: true,
                lag_ms: 15,
            }]
            .into_boxed_slice(),
            mods: mods(),
            ..Default::default()
        },
        48000.,
    )
    .unwrap();
    let mut state = ControlState::new(&plan);
    let cc = [101; 128];
    let input = inputs(&cc, 9);
    support::without_heap(|| {
        for cycle in 0..20 {
            state.reset(&plan, &input, cycle * 129, 48000.).unwrap();
            assert_eq!(state.start_frame(), cycle * 129);
            state.plan_controls(&plan, &input, 17);
            let (mut amp, mut vol, mut positions) = ([0.; 128], [0.; 128], [0; 128]);
            state
                .preview_pitch(&plan, 120., 0.9, &mut positions[..17])
                .unwrap();
            state
                .render(
                    &plan,
                    120.,
                    0.9,
                    &mut amp[..17],
                    None,
                    Some(&mut vol[..17]),
                    &mut positions[..17],
                )
                .unwrap();
            state.release(&plan);
            state.release(&plan);
            state.plan_controls(&plan, &input, 77);
            state.skip(&plan, 120., 0.9, 77).unwrap();
        }
    });
}
#[test]
fn admission_refuses_overflow_and_invalid_slots_without_dropping_sources() {
    assert!(
        ModTable::new(
            vec![Mod {
                route: Some((Source::Cc(128), Target::Pitch)),
                intensity: 1.,
                lag: 0.,
                curve: None
            }]
            .into_boxed_slice()
        )
        .is_err()
    );
    assert!(
        ModTable::new(
            vec![
                Mod {
                    route: Some((Source::Velocity, Target::Volume)),
                    intensity: 1.,
                    lag: 0.,
                    curve: None
                };
                9
            ]
            .into_boxed_slice()
        )
        .is_err()
    );
    let mut desc = ControlDescription::default();
    let mut src = source(0., 0.);
    src.slot = 16;
    desc.pitch_lfos = vec![src].into_boxed_slice();
    assert!(ControlPlan::prepare(desc, 48000.).is_err());
    let shared = source(0., 0.);
    let volume = VolumeLfo {
        source: PitchLfo {
            start_phase: 0.25,
            depth: 0.,
            targets: Vec::new(),
            ..shared.clone()
        },
        target: 0,
        intensity: 1.,
        negative: false,
        lag_ms: 0,
    };
    assert!(
        ControlPlan::prepare(
            ControlDescription {
                pitch_lfos: vec![shared].into_boxed_slice(),
                volume_lfos: vec![volume].into_boxed_slice(),
                ..Default::default()
            },
            48000.
        )
        .is_err()
    );
    assert!(
        ControlPlan::prepare(
            ControlDescription {
                volume_lfos: vec![VolumeLfo {
                    source: source(0., 1.),
                    target: 0,
                    intensity: 1.,
                    negative: false,
                    lag_ms: 0
                }]
                .into_boxed_slice(),
                ..Default::default()
            },
            48000.
        )
        .is_err()
    );
    let plan = ControlPlan::prepare(ControlDescription::default(), 48000.).unwrap();
    let mut state = ControlState::new(&plan);
    let cc = [0; 128];
    assert_eq!(
        state.reset(&plan, &inputs(&cc, 0), 0, 44100.),
        Err(Error::InvalidInput)
    );
    let mut amp = [0.; 129];
    let mut positions = [0; 129];
    assert_eq!(
        state.render(&plan, 120., 1., &mut amp, None, None, &mut positions),
        Err(Error::InvalidInput)
    );
    let mut amp = [0.; 1];
    let mut positions = [0; 1];
    state
        .render(&plan, 120., 1., &mut amp, None, None, &mut positions)
        .unwrap();
    assert_eq!(amp, [1.]);
}

#[test]
fn note_start_time_routes_and_zero_stages_match_literal_v1() {
    for native in [false, true] {
        for velocity in [0, 1, 63, 100, 127] {
            let table = ModTable::new(
                vec![
                    Mod {
                        route: Some((Source::Velocity, Target::Attack)),
                        intensity: -0.7,
                        lag: 0.,
                        curve: Some(Arc::new(std::array::from_fn(|i| {
                            1. - 0.41 * (i as f32 / 127.)
                        }))),
                    },
                    Mod {
                        route: Some((Source::Constant, Target::Release)),
                        intensity: 0.6,
                        lag: 0.,
                        curve: None,
                    },
                ]
                .into_boxed_slice(),
            )
            .unwrap();
            let plan = ControlPlan::prepare(
                ControlDescription {
                    amplitude: Ahdsr {
                        hold: 0.,
                        decay: 0.,
                        ..ahdsr(-0.6)
                    },
                    native_amplitude: native,
                    mods: table,
                    ..Default::default()
                },
                48000.,
            )
            .unwrap();
            let cc = [0; 128];
            let input = Inputs {
                velocity,
                ..inputs(&cc, 3)
            };
            let mut params = plan.description().amplitude;
            oracle::scale_envelope(&mut params, plan.description().mods.assignments(), &input);
            let mut reference = oracle::EnvelopeOracle::new(&params, native, false, 48000.);
            let mut state = ControlState::new(&plan);
            support::without_heap(|| state.reset(&plan, &input, 0, 48000.).unwrap());
            for block in 0..100 {
                if block == 17 {
                    state.release(&plan);
                    reference.release(None);
                }
                let n = [1, 7, 17, 32, 77, 128][block % 6];
                let (mut got, mut expected) = ([0.; 128], [0.; 128]);
                render(&mut state, &plan, n, &mut got, None, None);
                reference.render(&mut expected[..n], None, None, 48000.);
                bits(&got[..n], &expected[..n]);
            }
        }
        let zero = ControlPlan::prepare(
            ControlDescription {
                amplitude: Ahdsr {
                    attack: 0.,
                    hold: 0.,
                    decay: 0.,
                    sustain: 0.3,
                    release: 0.,
                    curve: 0.,
                    ahd_only: false,
                },
                native_amplitude: native,
                ..Default::default()
            },
            48000.,
        )
        .unwrap();
        let mut state = ControlState::new(&zero);
        let mut reference =
            oracle::EnvelopeOracle::new(&zero.description().amplitude, native, false, 48000.);
        for block in 0..5 {
            if block == 1 {
                state.release(&zero);
                reference.release(None);
            }
            let (mut got, mut expected) = ([0.; 128], [0.; 128]);
            render(&mut state, &zero, 128, &mut got, None, None);
            reference.render(&mut expected, None, None, 48000.);
            bits(&got, &expected);
            assert_eq!(state.done(), reference.done());
        }
    }
}

#[test]
fn addressed_filter_projection_matches_pinned_follow_and_control_ticks_without_heap() {
    let mods: Vec<_> = [
        Source::Cc(1),
        Source::Cc(74),
        Source::Bend,
        Source::Pressure,
        Source::Velocity,
        Source::Key,
        Source::Constant,
        Source::Counter,
    ]
    .into_iter()
    .map(|source| Mod {
        route: Some((source, Target::Module)),
        intensity: -0.3,
        lag: 0.007,
        curve: Some(Arc::new(std::array::from_fn(|i| (i as f32 / 127.).powi(2)))),
    })
    .collect();
    let mut raw = vec![Mod {
        route: None,
        intensity: 0.,
        lag: 0.,
        curve: None,
    }];
    raw.extend(mods);
    let table = ModTable::new(raw.into_boxed_slice()).unwrap();
    let mut cc = [0; 128];
    let mut values = [0.; 8];
    let mut expected_values = [0.; 8];
    for ((value, expected), m) in values
        .iter_mut()
        .zip(&mut expected_values)
        .zip(&table.assignments()[1..])
    {
        *value = m.start_value(&inputs(&cc, 0));
        *expected = oracle::filter_start(m, &inputs(&cc, 0));
    }
    let p = ahdsr(-0.6);
    let mut env = Envelope::new(&p, 48000.);
    let mut reference = oracle::EnvelopeOracle::new(&p, false, false, 48000.);
    let mut controls = [0.; 128];
    let mut expected_controls = [0.; 128];
    support::without_heap(|| {
        for block in 0..384 {
            cc[1] = (block % 128) as u8;
            let mut input = inputs(&cc, block as u32);
            input.cc74 = Some((127 - block % 128) as u8);
            input.bend = block as f32 / 192. - 1.;
            input.pressure = (block % 128) as u8;
            let n = [1, 7, 8, 9, 31, 32, 33, 77, 128][block % 9];
            for ((value, expected), m) in values
                .iter_mut()
                .zip(&mut expected_values)
                .zip(&table.assignments()[1..])
            {
                m.follow(value, &input, n, 48000.);
                oracle::filter_follow(m, expected, &input, n, 48000.);
                assert_eq!(value.to_bits(), expected.to_bits());
            }
            if block == 100 {
                env.release(None);
                reference.release(None);
            }
            // Pinned filter::process_chain uses CONTROL=32 and retains bypassed clocks.
            if matches!(env.phase(), Phase::Sustain | Phase::Done) {
                env.skip(n, None, 48000.);
                reference.skip(n, None, 48000.);
                controls[..n].fill(env.level());
                expected_controls[..n].fill(reference.level());
            } else {
                env.render(&mut controls[..n], None, 48000.);
                reference.render(&mut expected_controls[..n], None, None, 48000.);
            }
            for tick in (0..n).step_by(32) {
                assert_eq!(controls[tick].to_bits(), expected_controls[tick].to_bits());
                // A hold/process projection reads twice without advancing the source.
                for _ in 0..2 {
                    assert_eq!(env.level().to_bits(), reference.level().to_bits());
                    for (value, expected) in values.iter().zip(&expected_values) {
                        assert_eq!(value.to_bits(), expected.to_bits());
                    }
                }
            }
        }
    });
    assert!(table.assignments()[0].route.is_none());
    assert_eq!(
        table.assignments()[1].route,
        Some((Source::Cc(1), Target::Module))
    );
}

#[test]
fn arbitrary_stage_release_and_ahd_only_match_literal_v1() {
    for native in [false, true] {
        for ahd_only in [false, true] {
            // Ordinary/native stage counts differ; compare each consumer with its literal path.
            for release_frame in [0, 1, 31, 32, 33, 64, 65, 96, 127, 128, 160, 480] {
                let amplitude = Ahdsr {
                    attack: 0.001,
                    hold: 0.001,
                    decay: 0.001,
                    sustain: 0.3,
                    release: 0.001,
                    curve: 0.3,
                    ahd_only,
                };
                let plan = ControlPlan::prepare(
                    ControlDescription {
                        amplitude,
                        native_amplitude: native,
                        ..Default::default()
                    },
                    48000.,
                )
                .unwrap();
                let mut state = ControlState::new(&plan);
                let mut reference = oracle::EnvelopeOracle::new(&amplitude, native, false, 48000.);
                let mut amp = [0.; 128];
                let mut expected = [0.; 128];
                let mut frame = 0;
                for _ in 0..64 {
                    if frame == release_frame {
                        state.release(&plan);
                        reference.release(None);
                    }
                    let mut n = [1, 7, 31, 33, 128][frame % 5];
                    if frame < release_frame {
                        n = n.min(release_frame - frame);
                    }
                    render(&mut state, &plan, n, &mut amp, None, None);
                    reference.render(&mut expected[..n], None, None, 48000.);
                    bits(&amp[..n], &expected[..n]);
                    assert_eq!(
                        (state.phase(), state.done()),
                        (reference.phase(), reference.done())
                    );
                    frame += n;
                }
            }
        }
    }
}

#[test]
fn raw_ahdsr_forward_mapping_keeps_v1_native_setter_and_ordinary_conversion_distinct() {
    let mut source = sampler_ir::SourceAhdsr {
        group: 7,
        slot: 12,
        attack_ms: 125.012924,
        attack_curve: 0.75,
        hold_ms: 0.012345,
        decay_ms: 25000.043,
        sustain: 0.4,
        release_ms: 1234.567,
        ahd_only: true,
        native_amplitude: false,
    };
    let ordinary = Ahdsr::from(&source);
    let expected = Ahdsr {
        attack: source.attack_ms / 1000.,
        curve: source.attack_curve,
        hold: source.hold_ms / 1000.,
        decay: source.decay_ms / 1000.,
        sustain: source.sustain,
        release: source.release_ms / 1000.,
        ahd_only: false,
    };
    assert_eq!(
        [
            ordinary.attack,
            ordinary.curve,
            ordinary.hold,
            ordinary.decay,
            ordinary.sustain,
            ordinary.release
        ]
        .map(f32::to_bits),
        [
            expected.attack,
            expected.curve,
            expected.hold,
            expected.decay,
            expected.sustain,
            expected.release
        ]
        .map(f32::to_bits)
    );
    assert!(!ordinary.ahd_only);
    source.native_amplitude = true;
    let native = Ahdsr::from(&source);
    let expected = Ahdsr {
        attack: source.attack_ms * 0.001,
        hold: source.hold_ms * 0.001,
        decay: source.decay_ms * 0.001,
        release: source.release_ms * 0.001,
        ahd_only: true,
        ..expected
    };
    assert_eq!(
        [
            native.attack,
            native.curve,
            native.hold,
            native.decay,
            native.sustain,
            native.release
        ]
        .map(f32::to_bits),
        [
            expected.attack,
            expected.curve,
            expected.hold,
            expected.decay,
            expected.sustain,
            expected.release
        ]
        .map(f32::to_bits)
    );
    assert!(native.ahd_only);
    assert_ne!(
        native.attack.to_bits(),
        ordinary.attack.to_bits(),
        "this saved scalar distinguishes the setter laws"
    );
    let plan = ControlPlan::prepare(
        ControlDescription {
            amplitude: native,
            native_amplitude: true,
            ..ControlDescription::default()
        },
        48000.,
    )
    .unwrap();
    let mut actual = ControlState::new(&plan);
    let mut frozen = oracle::Amplitude::new(&expected, 48000., true);
    let mut out = [0.; 128];
    let mut reference = [0.; 128];
    support::without_heap(|| {
        render(&mut actual, &plan, 64, &mut out, None, None);
        frozen.render(&mut reference[..64], None, 48000.);
        bits(&out[..64], &reference[..64]);
        assert_eq!(actual.amplitude_level().to_bits(), frozen.level().to_bits());
    });
}

#[test]
fn original_flex_projection_uses_pinned_direction_curve_and_units() {
    use sampler_ir::kontakt::FlexPoint as Raw;
    let points = [Raw { time_ms: 125., level: 0.8, curve: 0.25 },
        Raw { time_ms: 25., level: 0.3, curve: 0.75 },
        Raw { time_ms: 0., level: 0.3, curve: 1. }];
    let flex = Flex::from_kontakt(&points, 1).unwrap();
    assert_eq!(flex.sustain, 1);
    assert_eq!(flex.points.iter().map(|p| (p.seconds.to_bits(), p.level.to_bits(), p.curve.to_bits())).collect::<Vec<_>>(),
        [(0.125f32.to_bits(), 0.8f32.to_bits(), (-0.5f32).to_bits()),
         (0.025f32.to_bits(), 0.3f32.to_bits(), (-0.5f32).to_bits()),
         (0f32.to_bits(), 0.3f32.to_bits(), 1f32.to_bits())]);
    assert!(Flex::from_kontakt(&[], 0).is_err());
    assert!(Flex::from_kontakt(&points, 3).is_err());
    assert!(Flex::from_kontakt(&[points[0]; 33], 0).is_err());
    for bad in [Raw { time_ms: f32::NAN, ..points[0] },
        Raw { time_ms: f32::INFINITY, ..points[0] }, Raw { time_ms: -1., ..points[0] },
        Raw { level: f32::NAN, ..points[0] }, Raw { level: 1.01, ..points[0] },
        Raw { level: -0.01, ..points[0] }, Raw { curve: f32::INFINITY, ..points[0] },
        Raw { curve: -0.01, ..points[0] }, Raw { curve: 1.01, ..points[0] }] {
        assert!(Flex::from_kontakt(&[bad], 0).is_err());
    }
}
