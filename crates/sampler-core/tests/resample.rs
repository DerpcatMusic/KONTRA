use sampler_core::{
    Direction, Envelope, Error, Event, Input, Limits, Loop, LoopMode, Pcm, Playback, Prepared,
    Protocol, Region, Runtime,
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
            let pcm = Pcm {
                rate: source_rate,
                frames: (0..4096)
                    .map(|i| {
                        let angle = 2.0 * PI * frequency * i as f64;
                        [angle.cos() as f32, angle.sin() as f32]
                    })
                    .collect(),
            };
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
                        Pcm {
                            rate: 24000,
                            frames: (0..10).map(|i| [i as f32; 2]).collect(),
                        },
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
        let pcm = Pcm {
            rate: 96000,
            frames: (0..4096)
                .map(|i| {
                    let x = 2.0 * PI * source_frequency * i as f64;
                    [x.cos() as f32, x.sin() as f32]
                })
                .collect(),
        };
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
                Pcm {
                    rate: 48000,
                    frames: Box::from([[1.0; 2]; 8])
                },
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
            vec![Pcm {
                rate: 48000,
                frames,
            }],
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
    let sample = || {
        vec![Pcm {
            rate: 96000,
            frames: Box::from([[0.25; 2]; 128]),
        }]
    };
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
