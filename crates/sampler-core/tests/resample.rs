use sampler_core::{
    Direction, Envelope, Error, Event, Expression, Inheritance, Input, Limits, Loop, LoopMode, Pcm,
    Playback, Prepared, Protocol, Region, Runtime,
};
use std::f64::consts::PI;
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}
fn prepare(pcm: Pcm, rate: u32, playback: Playback) -> Result<Prepared, Error> {
    Prepared::new(
        rate,
        vec![pcm],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
            envelope: Envelope::new(0, 0, 0, 1.0, 1000).unwrap(),
            playback,
        }],
        1,
    )
}
fn runtime(plan: Prepared) -> Runtime {
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 1,
            families: 4,
            decisions: 0,
            expressions: 4,
            voices: 4,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
        },
    )
    .unwrap()
}

#[test]
fn rates_and_transposition_follow_analytic_tones_without_heap_or_partition_drift() {
    for (source_rate, output_rate, semitones) in [
        (44100, 48000, 0.0),
        (48000, 44100, 0.0),
        (96000, 48000, 0.0),
        (48000, 96000, 12.0),
        (48000, 48000, -12.0),
        (48000, 48000, 7.0),
    ] {
        let frequency = 0.037;
        let step = f64::from(source_rate) / f64::from(output_rate) * (semitones / 12.0_f64).exp2();
        let mut baseline = [[0.0; 2]; 256];
        for partition in [1, 7, 64, 256] {
            let pcm = Pcm::new(
                source_rate,
                (0..4096)
                    .map(|i| {
                        let angle = 2.0 * PI * frequency * i as f64;
                        [angle.cos() as f32, angle.sin() as f32]
                    })
                    .collect(),
            )
            .unwrap();
            let mut rt = runtime(
                prepare(
                    pcm,
                    output_rate,
                    Playback {
                        start: 512,
                        transpose_semitones: semitones,
                        ..Playback::default()
                    },
                )
                .unwrap(),
            );
            let mut audio = [[0.0; 2]; 256];
            support::without_heap(|| {
                rt.trigger(input(), 60, 1.0).unwrap();
                // Skip the finite source-view boundary transient.
                rt.render(&mut [[0.0; 2]; 128]).unwrap();
                for block in audio.chunks_mut(partition) {
                    rt.render(&mut []).unwrap();
                    rt.render(block).unwrap();
                }
                rt.panic();
                rt.flush_ended(|_| true);
            });
            if partition == 1 {
                baseline = audio;
            } else {
                assert_eq!(audio, baseline);
            }
            for (i, frame) in audio.iter().enumerate() {
                let phase = 2.0 * PI * frequency * (512.0 + (128 + i) as f64 * step);
                assert!((f64::from(frame[0]) - phase.cos()).abs() < 0.0001);
                assert!((f64::from(frame[1]) - phase.sin()).abs() < 0.0001);
            }
        }
    }
}

// Independent direct trigonometric FIR oracle; no coefficient table or runtime
// cursor logic. The traversal sequences below are authored explicitly.
fn filtered(position: f64, mut read: impl FnMut(i64) -> f64) -> f32 {
    let base = position.floor() as i64;
    let fraction = position - position.floor();
    let (mut value, mut weight) = (0.0, 0.0);
    for offset in -48..=48 {
        let x = offset as f64 - fraction;
        let h = if x.abs() >= 48.0 {
            0.0
        } else {
            let sinc = if x == 0.0 {
                0.9
            } else {
                (0.9 * PI * x).sin() / (PI * x)
            };
            sinc * (0.42 + 0.5 * (PI * x / 48.0).cos() + 0.08 * (2.0 * PI * x / 48.0).cos())
        };
        weight += h;
        value += h * read(base + offset);
    }
    (value / weight) as f32
}

#[test]
fn fractional_loop_release_preserves_traversal_guards_and_source_end() {
    for direction in [Direction::Forward, Direction::Reverse] {
        for release in [8, 9, 10, 12] {
            let exit = if release == 8 { 4 } else { 6 };
            let mut baseline = [[0.0; 2]; 24];
            for partition in [1, 3, 8, 24] {
                let mut rt = runtime(
                    prepare(
                        Pcm::new(24000, (0..10).map(|i| [i as f32; 2]).collect()).unwrap(),
                        48000,
                        Playback {
                            start: 2,
                            end: Some(8),
                            direction,
                            loop_range: Some(Loop {
                                start: 4,
                                end: 6,
                                mode: LoopMode::UntilRelease,
                            }),
                            ..Playback::default()
                        },
                    )
                    .unwrap(),
                );
                let mut audio = [[0.0; 2]; 24];
                support::without_heap(|| {
                    let note = rt.trigger(input(), 60, 1.0).unwrap();
                    rt.schedule_event(release, Event::KeyUp(note)).unwrap();
                    for block in audio.chunks_mut(partition) {
                        rt.render(block).unwrap();
                    }
                    rt.flush_ended(|_| true);
                    assert_eq!((rt.voice_count(), rt.note_count()), (0, 0));
                });
                if partition == 1 {
                    baseline = audio;
                } else {
                    assert_eq!(baseline, audio);
                }
                for (i, frame) in audio.iter().enumerate() {
                    let released = i >= release as usize;
                    let position = i as f64 * 0.5;
                    let expected = if released && position >= f64::from(exit + 2) {
                        0.0
                    } else {
                        let source = filtered(position, |index| {
                            if index < 0 {
                                return 0.0;
                            }
                            let forward = if released && index >= i64::from(exit) {
                                if index >= i64::from(exit + 2) {
                                    return 0.0;
                                }
                                6 + index - i64::from(exit)
                            } else if index < 4 {
                                2 + index
                            } else {
                                4 + (index - 4) % 2
                            };
                            if direction == Direction::Forward {
                                forward as f64
                            } else {
                                (9 - forward) as f64
                            }
                        });
                        let level = if released {
                            (1.0 - (i - release as usize) as f64 / 1000.0) as f32
                        } else {
                            1.0
                        };
                        source * level
                    };
                    assert!(
                        (frame[0] - expected).abs() < 0.00002,
                        "{direction:?} release={release} frame={i}: {} != {expected}",
                        frame[0]
                    );
                    assert_eq!(frame[0], frame[1]);
                }
            }
        }
    }
}

#[test]
fn downsampling_rejects_above_output_nyquist_and_invalid_ratios() {
    for source_frequency in [0.26, 0.3, 0.4, 0.49] {
        let pcm = Pcm::new(
            96000,
            (0..4096)
                .map(|i| {
                    let x = 2.0 * PI * source_frequency * i as f64;
                    [x.cos() as f32, x.sin() as f32]
                })
                .collect(),
        )
        .unwrap();
        let mut rt = runtime(prepare(pcm, 48000, Playback::default()).unwrap());
        rt.trigger(input(), 60, 1.0).unwrap();
        rt.render(&mut [[0.0; 2]; 128]).unwrap();
        let mut audio = [[0.0; 2]; 256];
        rt.render(&mut audio).unwrap();
        assert!(audio.iter().flatten().all(|x| x.abs() < 0.0002));
    }
    for transpose_semitones in [f64::NAN, f64::INFINITY, f64::MAX, -f64::MAX, 49.0, -97.0] {
        assert!(matches!(
            prepare(
                Pcm::new(48000, Box::from([[1.0; 2]; 8])).unwrap(),
                48000,
                Playback {
                    transpose_semitones,
                    ..Playback::default()
                }
            ),
            Err(Error::InvalidInput)
        ));
    }
}

#[test]
fn prepared_key_tracking_uses_logical_keys_and_keeps_exact_root_pitch() {
    for key in [59, 60, 61, 72] {
        let frames: Box<[_]> = (0..4096)
            .map(|i| {
                let phase = 2.0 * PI * 0.037 * i as f64;
                [phase.cos() as f32, phase.sin() as f32]
            })
            .collect();
        let reference = frames.clone();
        let plan = Prepared::new(
            48000,
            vec![Pcm::new(48000, frames).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 59,
                key_high: 72,
                root_key: Some(60),
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 1.0,
                envelope: Envelope::default(),
                playback: Playback {
                    start: 512,
                    ..Playback::default()
                },
            }],
            14,
        )
        .unwrap();
        let mut rt = runtime(plan);
        let mut audio = [[0.0; 2]; 256];
        support::without_heap(|| {
            // The physical address stays key 60 even when the musical key differs.
            let note = rt.trigger(input(), key, 1.0).unwrap();
            rt.render(&mut [[0.0; 2]; 128]).unwrap();
            rt.render(&mut audio).unwrap();
            assert_eq!(rt.note_off(input()), Ok(note));
            rt.flush_ended(|ended| {
                assert_eq!(ended, input());
                true
            });
            assert_eq!(rt.note_count(), 0);
        });
        if key == 60 {
            assert_eq!(
                audio.as_slice(),
                &reference[640..896],
                "root note lost direct-read exactness"
            );
        } else {
            let step = ((f64::from(key) - 60.0) / 12.0).exp2();
            for (i, frame) in audio.iter().enumerate() {
                let phase = 2.0 * PI * 0.037 * (512.0 + (128 + i) as f64 * step);
                assert!((f64::from(frame[0]) - phase.cos()).abs() < 0.0001);
                assert!((f64::from(frame[1]) - phase.sin()).abs() < 0.0001);
            }
        }
    }
    let region = Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: Some(72),
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
        envelope: Envelope::default(),
        playback: Playback {
            transpose_semitones: 48.0,
            ..Playback::default()
        },
    };
    // Only mapped keys need to be playable; the unselected root can lie outside
    // the supported source/output ratio, while the authored key is exactly step 16.
    let sample = || vec![Pcm::new(96000, Box::from([[0.25; 2]; 128])).unwrap()];
    assert!(Prepared::new(48000, sample(), vec![region], 1).is_ok());
    assert!(matches!(
        Prepared::new(
            48000,
            sample(),
            vec![Region {
                key_high: 61,
                ..region
            }],
            2
        ),
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        Prepared::new(
            48000,
            sample(),
            vec![Region {
                root_key: Some(128),
                ..region
            }],
            1
        ),
        Err(Error::InvalidInput)
    ));
}

fn expression(pitch_semitones: f64) -> Expression {
    Expression {
        pitch_semitones,
        ..Expression::default()
    }
}
fn tone() -> Pcm {
    Pcm::new(
        48000,
        (0..4096)
            .map(|i| {
                let phase = 2.0 * PI * 0.017 * i as f64;
                [phase.cos() as f32, phase.sin() as f32]
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn native_tuning_tracks_logical_pitch_without_changing_note_identity() {
    use sampler_core::Tuning;
    let mut offsets = [0.0; 128];
    // Deliberately nonmonotonic: key mapping must not assume ascending pitches.
    offsets[59] = 2.25;
    offsets[60] = -0.5;
    offsets[61] = -1.75;
    let tuning = Tuning::new(offsets).unwrap();
    assert_eq!(tuning.offsets_semitones(), &offsets);
    for root in [Some(60), None] {
        for key in 59..=61 {
            let mut baseline = [[0.0; 2]; 256];
            for partition in [1, 7, 64, 256] {
                let mut rt = runtime(
                    Prepared::new_tuned(
                        48000,
                        vec![tone()],
                        vec![Region {
                            sample: 0,
                            key_low: 59,
                            key_high: 61,
                            root_key: root,
                            velocity_low: 0.0,
                            velocity_high: 1.0,
                            gain: 1.0,
                            envelope: Envelope::default(),
                            playback: Playback {
                                start: 512,
                                ..Playback::default()
                            },
                        }],
                        3,
                        &tuning,
                    )
                    .unwrap(),
                );
                let mut audio = [[0.0; 2]; 256];
                support::without_heap(|| {
                    let note = rt
                        .trigger_with_expression(input(), key, 1.0, expression(0.25))
                        .unwrap();
                    assert_eq!(rt.note(note).unwrap().0, key);
                    rt.schedule_event(128, Event::Expression(note, expression(-0.25)))
                        .unwrap();
                    for block in audio.chunks_mut(partition) {
                        rt.render(block).unwrap();
                    }
                    assert_eq!(rt.note_off(input()), Ok(note));
                    rt.flush_ended(|ended| {
                        assert_eq!(ended, input());
                        true
                    });
                    assert_eq!(rt.note_count(), 0);
                });
                if partition == 1 {
                    baseline = audio;
                } else {
                    assert_eq!(audio, baseline);
                }
                let base = if root.is_some() {
                    f64::from(key) - 60.0 + offsets[key as usize]
                } else {
                    0.0
                };
                let mut position = 512.0;
                for (i, frame) in audio.iter().enumerate() {
                    let phase = 2.0 * PI * 0.017 * position;
                    // The source view is zero-extended before its start; compare
                    // the steady tone after the filter no longer touches that edge.
                    if i >= 64 {
                        assert!((f64::from(frame[0]) - phase.cos()).abs() < 0.0001);
                        assert!((f64::from(frame[1]) - phase.sin()).abs() < 0.0001);
                    }
                    position += ((base + if i < 128 { 0.25 } else { -0.25 }) / 12.0).exp2();
                }
            }
        }
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        offsets[127] = invalid;
        assert!(matches!(Tuning::new(offsets), Err(Error::InvalidInput)));
    }
    let region = Region {
        sample: 0,
        key_low: 60,
        key_high: 61,
        root_key: Some(60),
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    // A later candidate, not only the first key, must pass source-rate validation.
    for invalid in [-1000.0, 1000.0, f64::MAX] {
        let mut offsets = [0.0; 128];
        offsets[61] = invalid;
        let tuning = Tuning::new(offsets).unwrap();
        assert!(matches!(
            Prepared::new_tuned(48000, vec![tone()], vec![region], 2, &tuning),
            Err(Error::InvalidInput)
        ));
        assert!(
            Prepared::new_tuned(
                48000,
                vec![tone()],
                vec![Region {
                    key_high: 60,
                    ..region
                }],
                1,
                &tuning
            )
            .is_ok()
        );
        assert!(
            Prepared::new_tuned(
                48000,
                vec![tone()],
                vec![Region {
                    root_key: None,
                    ..region
                }],
                2,
                &tuning
            )
            .is_ok()
        );
    }
}

#[test]
fn absolute_pitch_overrides_tuning_and_survives_generated_note_transposition() {
    use sampler_core::{Duration, Inheritance, Instruction, NotePitch, Program, Tuning, Velocity};
    for absolute in [0.0, 60.0 + 1.0 / 512.0, 60.25, 127.0 + 511.0 / 512.0] {
        let key = absolute as u8;
        let transpose = if key == 127 { -1 } else { 1 };
        for generated in [false, true] {
            let region = Region {
                sample: 0,
                key_low: key.min((i16::from(key) + i16::from(transpose)) as u8),
                key_high: key.max((i16::from(key) + i16::from(transpose)) as u8),
                root_key: Some(key),
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 1.0,
                envelope: Envelope::default(),
                playback: Playback {
                    start: 512,
                    ..Playback::default()
                },
            };
            let mut plan = Prepared::new_tuned(
                48000,
                vec![tone()],
                vec![region],
                2,
                &Tuning::new([12.0; 128]).unwrap(),
            )
            .unwrap();
            if generated {
                plan = plan
                    .with_programs(
                        vec![
                            Program::new(vec![
                                Instruction::Wait(3),
                                Instruction::Play {
                                    transpose,
                                    velocity: Velocity::Fixed(1.0),
                                    inheritance: Inheritance::Independent,
                                    duration: Duration::Gate,
                                },
                                Instruction::End,
                            ])
                            .unwrap(),
                        ],
                        Some(0),
                    )
                    .unwrap();
            }
            let mut rt = Runtime::new(
                plan,
                Limits {
                    notes: 4,
                    channels: 1,
                    families: 4,
                    decisions: 0,
                    expressions: 4,
                    voices: 4,
                    commands: 4,
                    behaviors: 1,
                    behavior_fuel: 8,
                    behavior_cells: 0,
                },
            )
            .unwrap();
            let mut reference = runtime(
                prepare(
                    tone(),
                    48000,
                    Playback {
                        start: 512,
                        transpose_semitones: absolute - f64::from(key)
                            + if generated { f64::from(transpose) } else { 0.0 },
                        ..Playback::default()
                    },
                )
                .unwrap(),
            );
            let mut actual = [[0.0; 2]; 256];
            let mut expected = actual;
            support::without_heap(|| {
                let n = rt
                    .trigger_pitched(
                        input(),
                        NotePitch::Absolute(absolute),
                        1.0,
                        expression(0.25),
                    )
                    .unwrap();
                assert_eq!(rt.note_pitch(n), Ok(NotePitch::Absolute(absolute)));
                assert_eq!(rt.note(n).unwrap().0, key);
                if generated {
                    rt.render(&mut [[0.0; 2]; 3]).unwrap();
                }
                // Independent expression resets the bend, not the inherent pitch.
                reference
                    .trigger_with_expression(
                        input(),
                        60,
                        1.0,
                        expression(if generated { 0.0 } else { 0.25 }),
                    )
                    .unwrap();
                for (a, b) in actual.chunks_mut(7).zip(expected.chunks_mut(7)) {
                    rt.render(a).unwrap();
                    reference.render(b).unwrap();
                }
                assert_eq!(actual, expected);
                rt.note_off(input()).unwrap();
                rt.flush_behaviors(|_, _, _| true);
                rt.flush_ended(|_| true);
                assert_eq!(rt.note_count(), 0);
            });
        }
    }
    let mut rt = runtime(prepare(tone(), 48000, Playback::default()).unwrap());
    support::without_heap(|| {
        for pitch in [f64::NAN, f64::INFINITY, -0.001, 128.0] {
            assert_eq!(
                rt.trigger_pitched(
                    input(),
                    NotePitch::Absolute(pitch),
                    1.0,
                    Expression::default()
                ),
                Err(Error::InvalidInput)
            );
            assert_eq!(
                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                (0, 0, 0)
            );
        }
    });
}

#[test]
fn live_pitch_is_phase_continuous_and_sample_accurate_at_every_partition() {
    let mut baseline = [[0.0; 2]; 512];
    for partition in [1, 7, 64, 256, 512] {
        let mut rt = runtime(
            prepare(
                tone(),
                48000,
                Playback {
                    start: 512,
                    ..Playback::default()
                },
            )
            .unwrap(),
        );
        let mut audio = [[0.0; 2]; 512];
        support::without_heap(|| {
            let note = rt.trigger(input(), 60, 1.0).unwrap();
            for (at, pitch) in [(256, 12.0), (384, -12.0), (511, 0.0)] {
                rt.schedule_event(at, Event::Expression(note, expression(pitch)))
                    .unwrap();
            }
            rt.render(&mut [[0.0; 2]; 128]).unwrap();
            for block in audio.chunks_mut(partition) {
                rt.render(&mut []).unwrap();
                rt.render(block).unwrap();
            }
            assert_eq!(rt.pending_commands(), 0);
            rt.panic();
            rt.flush_ended(|_| true);
        });
        if partition == 1 {
            baseline = audio;
        } else {
            assert_eq!(baseline, audio);
        }
        let mut position = 640.0;
        for (index, frame) in audio.iter().enumerate() {
            let time = 128 + index;
            let phase = 2.0 * PI * 0.017 * position;
            assert!(
                (f64::from(frame[0]) - phase.cos()).abs() < 0.0001,
                "time={time}"
            );
            assert!(
                (f64::from(frame[1]) - phase.sin()).abs() < 0.0001,
                "time={time}"
            );
            position += match time {
                0..256 => 1.0,
                256..384 => 2.0,
                384..511 => 0.5,
                _ => 1.0,
            };
        }
    }
}

#[test]
fn pitch_inheritance_and_reused_channels_do_not_retarget_existing_sources() {
    for policy in [
        Inheritance::Linked,
        Inheritance::Snapshot,
        Inheritance::Independent,
    ] {
        let mut rt = runtime(prepare(tone(), 48000, Playback::default()).unwrap());
        let mut audio = [[0.0; 2]; 384];
        support::without_heap(|| {
            let root = rt.note_on(input(), 60, 1.0).unwrap();
            let owner = rt.expression_id(root).unwrap();
            rt.set_expression(owner, expression(12.0)).unwrap();
            let child = rt.child(root, 60, 1.0, false, policy).unwrap();
            rt.start(child, 0, 0, 1.0).unwrap();
            rt.schedule_event(256, Event::Expression(root, expression(-12.0)))
                .unwrap();
            rt.render(&mut [[0.0; 2]; 128]).unwrap();
            let reused = rt.note_on(input(), 60, 1.0).unwrap();
            rt.set_expression(rt.expression_id(reused).unwrap(), expression(36.0))
                .unwrap();
            rt.render(&mut audio).unwrap();
            rt.panic();
            rt.flush_ended(|_| true);
        });
        let first_step = if policy == Inheritance::Independent {
            1.0
        } else {
            2.0
        };
        let mut position = 128.0 * first_step;
        for (i, frame) in audio.iter().enumerate() {
            let phase = 2.0 * PI * 0.017 * position;
            assert!((f64::from(frame[0]) - phase.cos()).abs() < 0.0001);
            assert!((f64::from(frame[1]) - phase.sin()).abs() < 0.0001);
            position += if policy == Inheritance::Linked && i + 128 >= 256 {
                0.5
            } else {
                first_step
            };
        }
    }
}

#[test]
fn pending_pitch_constrains_late_sources_and_detachment_keeps_admission_atomic() {
    let mut rt = runtime(prepare(tone(), 48000, Playback::default()).unwrap());
    support::without_heap(|| {
        let root = rt.note_on(input(), 60, 1.0).unwrap();
        let owner = rt.expression_id(root).unwrap();
        rt.start(root, 0, 0, 1.0).unwrap();
        rt.schedule_event(100, Event::Expression(root, expression(12.0)))
            .unwrap();
        let family = rt.create_family(root).unwrap();
        let at_limit = Playback {
            transpose_semitones: 48.0,
            ..Playback::default()
        };
        assert_eq!(
            rt.start_family(family, 0, 50, 1.0, Envelope::default(), at_limit),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            (
                rt.voice_count(),
                rt.pending_commands(),
                rt.family_voice_count(family).unwrap()
            ),
            (1, 1, 0)
        );
        rt.start_family(
            family,
            0,
            50,
            1.0,
            Envelope::default(),
            Playback {
                transpose_semitones: 36.0,
                ..Playback::default()
            },
        )
        .unwrap();
        rt.finish_family(family).unwrap();
        // Delayed voices already constrain immediate and future expression.
        assert_eq!(
            rt.set_expression(owner, expression(13.0)),
            Err(Error::InvalidInput)
        );
        assert_eq!(
            rt.schedule_event(75, Event::Expression(root, expression(13.0))),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.expression(owner), Ok(Expression::default()));
        assert_eq!(rt.pending_commands(), 2);
        let detached = rt.child(root, 60, 1.0, false, Inheritance::Linked).unwrap();
        rt.detach_expression(detached).unwrap();
        let detached_family = rt.create_family(detached).unwrap();
        // The queued root expression no longer targets this detached owner.
        rt.start_family(detached_family, 0, 0, 1.0, Envelope::default(), at_limit)
            .unwrap();
        rt.finish_family(detached_family).unwrap();
        rt.render(&mut [[0.0; 2]; 128]).unwrap();
        assert_eq!(rt.expression(owner), Ok(expression(12.0)));
        assert_eq!(
            rt.expression(rt.expression_id(detached).unwrap()),
            Ok(Expression::default())
        );
        assert_eq!(rt.pending_commands(), 0);
        rt.panic();
        rt.flush_ended(|_| true);
        assert_eq!((rt.voice_count(), rt.note_count()), (0, 0));
    });
}

#[test]
fn initial_expression_precedes_selection_and_immediate_snapshot_programs() {
    use sampler_core::{Duration, Instruction, Outcome, Program, Velocity};
    let initial = Expression {
        gain: 0.5,
        pan: 0.25,
        pitch_semitones: 12.0,
        pressure: 0x1234_5678,
        timbre: 0xfedc_ba98,
    };
    let mut plain = runtime(prepare(tone(), 48000, Playback::default()).unwrap());
    let plan = prepare(tone(), 48000, Playback::default())
        .unwrap()
        .with_programs(
            vec![
                Program::new(vec![
                    Instruction::Play {
                        transpose: 0,
                        velocity: Velocity::Fixed(1.0),
                        inheritance: Inheritance::Snapshot,
                        duration: Duration::Frames(512),
                    },
                    Instruction::End,
                ])
                .unwrap(),
            ],
            Some(0),
        )
        .unwrap();
    let mut scripted = Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 1,
            families: 4,
            decisions: 0,
            expressions: 4,
            voices: 4,
            commands: 4,
            behaviors: 1,
            behavior_fuel: 4,
            behavior_cells: 0,
        },
    )
    .unwrap();
    let mut expected = [[0.0; 2]; 256];
    let mut actual = expected;
    support::without_heap(|| {
        for invalid in [
            expression(f64::NAN),
            expression(f64::INFINITY),
            expression(49.0),
            Expression {
                gain: -1.0,
                ..initial
            },
            Expression {
                pan: 2.0,
                ..initial
            },
        ] {
            assert_eq!(
                plain.trigger_with_expression(input(), 60, 1.0, invalid),
                Err(Error::InvalidInput)
            );
            assert_eq!(
                (
                    plain.note_count(),
                    plain.expression_count(),
                    plain.family_count(),
                    plain.voice_count()
                ),
                (0, 0, 0, 0)
            );
        }
        let direct = plain
            .trigger_with_expression(input(), 60, 1.0, initial)
            .unwrap();
        assert_eq!(
            plain.expression(plain.expression_id(direct).unwrap()),
            Ok(initial)
        );
        let root = scripted
            .trigger_with_expression(input(), 60, 1.0, initial)
            .unwrap();
        let owner = scripted.expression_id(root).unwrap();
        assert_eq!(scripted.expression(owner), Ok(initial));
        assert_eq!((scripted.note_count(), scripted.voice_count()), (2, 1));
        scripted
            .set_expression(owner, Expression::default())
            .unwrap();
        scripted.flush_behaviors(|_, note, outcome| {
            assert_eq!((note, outcome), (root, Outcome::Finished));
            true
        });
        plain.render(&mut expected).unwrap();
        scripted.render(&mut actual).unwrap();
        assert_eq!(actual, expected);
        for rt in [&mut plain, &mut scripted] {
            rt.panic();
            rt.flush_ended(|_| true);
            assert_eq!(
                (
                    rt.note_count(),
                    rt.expression_count(),
                    rt.family_count(),
                    rt.voice_count()
                ),
                (0, 0, 0, 0)
            );
        }
    });
    for (index, frame) in actual.iter().enumerate().skip(128) {
        let phase = 2.0 * PI * 0.017 * (index * 2) as f64;
        assert!((f64::from(frame[0]) - phase.cos() * 0.375).abs() < 0.0001);
        assert!((f64::from(frame[1]) - phase.sin() * 0.5).abs() < 0.0001);
    }
}

#[test]
fn controller_batches_reject_every_change_if_any_owner_or_source_is_invalid() {
    let mut rt = runtime(prepare(tone(), 48000, Playback::default()).unwrap());
    let mut foreign = runtime(prepare(tone(), 48000, Playback::default()).unwrap());
    support::without_heap(|| {
        let a = rt.trigger(input(), 60, 1.0).unwrap();
        let b = rt.trigger(input(), 60, 1.0).unwrap();
        let a = rt.expression_id(a).unwrap();
        let b = rt.expression_id(b).unwrap();
        let other = foreign.note_on(input(), 60, 1.0).unwrap();
        let other = foreign.expression_id(other).unwrap();
        for (bad_owner, bad_value, error) in [
            (b, expression(49.0), Error::InvalidInput),
            (b, expression(f64::NAN), Error::InvalidInput),
            (other, expression(12.0), Error::StaleHandle),
        ] {
            assert_eq!(
                rt.set_expressions(&[(a, expression(12.0)), (bad_owner, bad_value)]),
                Err(error)
            );
            assert_eq!(rt.expression(a), Ok(Expression::default()));
            assert_eq!(rt.expression(b), Ok(Expression::default()));
        }
        assert_eq!(
            rt.set_expressions(&[(a, expression(12.0)), (a, expression(7.0))]),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.expression(a), Ok(Expression::default()));
        rt.set_expressions(&[(a, expression(7.0)), (b, expression(-12.0))])
            .unwrap();
        assert_eq!(rt.expression(a), Ok(expression(7.0)));
        assert_eq!(rt.expression(b), Ok(expression(-12.0)));
        rt.render(&mut [[0.0; 2]; 64]).unwrap();
        for runtime in [&mut rt, &mut foreign] {
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

#[test]
fn muted_sources_match_audible_phase_envelopes_and_loop_exits_when_restored() {
    for direction in [Direction::Forward, Direction::Reverse] {
        for mode in [LoopMode::Continuous, LoopMode::UntilRelease] {
            for transpose_semitones in [0.0, -12.0, 7.0, 48.0] {
                for partition in [1, 11, 128] {
                    let make = || {
                        runtime(
                            Prepared::new(
                                48000,
                                vec![tone()],
                                vec![Region {
                                    sample: 0,
                                    key_low: 60,
                                    key_high: 60,
                                    root_key: None,
                                    velocity_low: 0.0,
                                    velocity_high: 1.0,
                                    gain: 1.0,
                                    envelope: Envelope::new(7, 3, 9, 0.4, 80).unwrap(),
                                    playback: Playback {
                                        start: 4,
                                        end: Some(256),
                                        direction,
                                        loop_range: Some(Loop {
                                            start: 16,
                                            end: 24,
                                            mode,
                                        }),
                                        transpose_semitones,
                                    },
                                }],
                                1,
                            )
                            .unwrap(),
                        )
                    };
                    let mut muted = make();
                    let mut audible = make();
                    let mut actual = [[0.0; 2]; 128];
                    let mut expected = actual;
                    support::without_heap(|| {
                        let note = muted
                            .trigger_with_expression(
                                input(),
                                60,
                                1.0,
                                Expression {
                                    gain: 0.0,
                                    ..Expression::default()
                                },
                            )
                            .unwrap();
                        let reference = audible.trigger(input(), 60, 1.0).unwrap();
                        let owner = muted.expression_id(note).unwrap();
                        let mut silent = true;
                        let mut begin = 0;
                        for (end, control) in [
                            (7, 1),
                            (23, 0),
                            (37, 1),
                            (42, -1),
                            (53, 0),
                            (60, 1),
                            (128, 2),
                        ] {
                            for (a, b) in actual[begin..end]
                                .chunks_mut(partition)
                                .zip(expected[begin..end].chunks_mut(partition))
                            {
                                muted.render(a).unwrap();
                                audible.render(b).unwrap();
                                if silent {
                                    b.fill([0.0; 2]);
                                }
                                assert_eq!(
                                    a, b,
                                    "{direction:?}/{mode:?}/{transpose_semitones}/{partition}"
                                );
                            }
                            assert_eq!(muted.voice_count(), audible.voice_count());
                            if control == -1 {
                                muted.key_up(note).unwrap();
                                audible.key_up(reference).unwrap();
                            } else if control < 2 {
                                silent = control == 0;
                                muted
                                    .set_expression(
                                        owner,
                                        Expression {
                                            gain: f64::from(control),
                                            ..Expression::default()
                                        },
                                    )
                                    .unwrap();
                            }
                            begin = end;
                        }
                        for rt in [&mut muted, &mut audible] {
                            rt.flush_ended(|_| true);
                            assert_eq!(
                                (rt.note_count(), rt.expression_count(), rt.voice_count()),
                                (0, 0, 0)
                            );
                        }
                    });
                }
            }
        }
    }
}
