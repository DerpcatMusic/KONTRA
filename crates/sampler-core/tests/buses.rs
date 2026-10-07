use sampler_core::*;
mod support;

fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 8,
        voices: 8,
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
fn plan(samples: Vec<Frame>, regions: usize) -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, samples.into_boxed_slice()).unwrap()],
        (0..regions)
            .map(|_| Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            })
            .collect(),
        regions,
    )
    .unwrap()
}
fn send(bus: Option<usize>, gain: f64) -> BusSend {
    BusSend { bus, gain }
}
fn filter() -> Processor {
    Processor::Biquad(Biquad::new(48000, FilterKind::LowPass, 12000., 1.).unwrap())
}
fn filtered(tail: u32) -> Bus {
    Bus {
        processors: vec![filter()],
        sends: vec![send(None, 1.)],
        tail_frames: tail,
    }
}
fn near(actual: Frame, expected: Frame) {
    for (a, b) in actual.into_iter().zip(expected) {
        assert!((a - b).abs() < 2e-7, "{actual:?} != {expected:?}");
    }
}
fn impulse(index: usize) -> f32 {
    // Independent direct-form recurrence for f=Fs/4, Q=1: a1=0, a2=1/3.
    let mut y = [0f64; 2];
    for i in 0..=index {
        let x = match i {
            0 | 2 => 1.,
            1 => 2.,
            _ => 0.,
        };
        let next = (x - y[0]) / 3.;
        y = [y[1], next];
    }
    y[1] as f32
}

#[test]
fn summed_bus_dag_fanout_and_tails_are_sample_exact_across_block_partitions() {
    for block in [1, 7, 64, 129] {
        let plan = plan(vec![[1., -0.5]], 1)
            .with_buses(
                vec![
                    Bus {
                        processors: vec![Processor::Gain(2.)],
                        sends: vec![send(Some(1), 1.)],
                        tail_frames: 2,
                    },
                    Bus {
                        processors: vec![Processor::Gain(3.)],
                        sends: vec![send(None, 1.)],
                        tail_frames: 1,
                    },
                    Bus {
                        processors: vec![filter()],
                        sends: vec![send(Some(0), 0.25), send(None, 0.5)],
                        tail_frames: 8,
                    },
                ],
                vec![Some(2)],
            )
            .unwrap();
        let mut rt = Runtime::new(plan, limits()).unwrap();
        let mut audio = [[0.; 2]; 140];
        support::without_heap(|| {
            rt.trigger(input(1), 60, 1.).unwrap();
            rt.trigger(input(2), 60, 0.5).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (i, frame) in audio.iter().enumerate() {
                let value = if i < 9 { 3. * impulse(i) } else { 0. };
                near(*frame, [value, -0.5 * value]);
            }
            assert_eq!(rt.voice_count(), 0);
            rt.note_off(input(1), None).unwrap();
            rt.note_off(input(2), None).unwrap();
            rt.flush_ended(|_| true);
            assert_eq!(rt.note_count(), 0);
        });
    }
}

#[test]
fn bus_tail_retains_only_its_generation_after_host_note_retirement() {
    let prepared = plan(vec![[1., -0.5]], 1)
        .with_buses(vec![filtered(8)], vec![Some(0)])
        .unwrap();
    let (mut rt, mut control) = Runtime::with_plan_updates(prepared, limits(), 2, 1).unwrap();
    let old = rt.active_plan();
    let request = control
        .submit(Box::new(plan(vec![[0.25; 2]; 32], 1)))
        .unwrap();
    support::without_heap(|| {
        rt.trigger(input(1), 60, 1.).unwrap();
        let mut onset = [[0.; 2]; 1];
        rt.render(&mut onset).unwrap();
        rt.note_off(input(1), None).unwrap();
        let mut ends = 0;
        rt.flush_ended(|ended| {
            assert_eq!(ended, input(1));
            ends += 1;
            true
        });
        assert_eq!((ends, rt.note_count(), rt.voice_count()), (1, 0, 0));
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        assert_ne!(rt.active_plan(), old);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.trigger(input(2), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 8];
        rt.render(&mut audio).unwrap();
        for (i, frame) in audio.iter().enumerate() {
            let value = impulse(i + 1);
            near(*frame, [0.25 + value, 0.25 - value * 0.5]);
        }
        assert_eq!(rt.collect_retired_plans(), 1);
        assert_eq!(rt.plan_count(), 1);
        let mut after = [[0.; 2]; 1];
        rt.render(&mut after).unwrap();
        assert_eq!(after[0], [0.25; 2]);
    });
    assert_eq!(control.retired().unwrap().request, 0);
}

#[test]
fn voice_and_bus_gains_share_control_values_without_sharing_scope_or_clock() {
    const LEVEL: ControlId = ControlId(7);
    for block in [1, 7, 129] {
        let gain = || {
            Processor::ControlGain(ControlRange {
                control: LEVEL,
                low: 0.,
                high: 1.,
                ramp_frames: 4,
            })
        };
        // Reversed builder order must preserve both independently compiled bindings.
        let prepared = plan(vec![[1.; 2]; 140], 1)
            .with_controls(vec![ControlDefinition {
                id: LEVEL,
                domain: ControlDomain::Real { min: 0., max: 1. },
                default: ControlValue::Real(0.),
            }])
            .unwrap()
            .with_buses(
                vec![Bus {
                    processors: vec![gain()],
                    sends: vec![send(None, 1.)],
                    tail_frames: 0,
                }],
                vec![Some(0)],
            )
            .unwrap()
            .with_voice_chains(
                vec![VoiceChain::new(vec![], vec![gain()], 0).unwrap()],
                vec![Some(0)],
            )
            .unwrap();
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        let mut audio = [[0.; 2]; 140];
        support::without_heap(|| {
            rt.trigger(input(1), 60, 1.).unwrap();
            rt.trigger(input(2), 60, 1.).unwrap();
            rt.schedule_event(
                1,
                Event::Control(
                    rt.active_plan(),
                    ControlWrite {
                        id: LEVEL,
                        value: ControlValue::Real(1.),
                    },
                ),
            )
            .unwrap();
            rt.schedule_event(
                70,
                Event::Control(
                    rt.active_plan(),
                    ControlWrite {
                        id: LEVEL,
                        value: ControlValue::Real(0.),
                    },
                ),
            )
            .unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (i, frame) in audio.iter().enumerate() {
                let gain = if i < 70 {
                    (i.saturating_sub(1) as f32 / 4.).min(1.)
                } else {
                    1. - ((i - 70) as f32 / 4.).min(1.)
                };
                near(*frame, [2. * gain * gain; 2]);
            }
        });
    }
}

#[test]
fn bus_faults_are_contained_and_panic_clears_shared_histories_and_tails() {
    let prepared = plan(vec![[1.; 2]], 2)
        .with_buses(
            vec![
                Bus {
                    processors: vec![Processor::Gain(f64::MAX), filter()],
                    sends: vec![send(None, 1.)],
                    tail_frames: 8,
                },
                filtered(8),
            ],
            vec![Some(0), Some(1)],
        )
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    support::without_heap(|| {
        rt.trigger(input(1), 60, 1.).unwrap();
        let mut onset = [[0.; 2]; 1];
        rt.render(&mut onset).unwrap();
        near(onset[0], [impulse(0); 2]);
        assert_eq!(rt.nonfinite_frames(), 1);
        rt.panic();
        let mut silence = [[1.; 2]; 16];
        rt.render(&mut silence).unwrap();
        assert_eq!(silence, [[0.; 2]; 16]);
        rt.flush_ended(|_| true);
        rt.trigger(input(2), 60, 1.).unwrap();
        rt.render(&mut onset).unwrap();
        near(onset[0], [impulse(0); 2]);
    });
}

#[test]
fn invalid_bus_graphs_and_controls_fail_before_publication() {
    let bus = |sends| Bus {
        processors: vec![],
        sends,
        tail_frames: 0,
    };
    for buses in [
        vec![bus(vec![send(Some(0), 0.)])],
        vec![bus(vec![send(Some(1), 1.)]), bus(vec![send(Some(0), 1.)])],
        vec![bus(vec![send(Some(1), 1.)])],
        vec![bus(vec![send(None, f64::NAN)])],
        vec![Bus {
            processors: vec![Processor::Gain(f64::INFINITY)],
            sends: vec![],
            tail_frames: 0,
        }],
        vec![Bus {
            processors: vec![Processor::Biquad(
                Biquad::new(96000, FilterKind::LowPass, 12000., 1.).unwrap(),
            )],
            sends: vec![],
            tail_frames: 0,
        }],
        vec![Bus {
            processors: vec![Processor::ControlGain(ControlRange {
                control: ControlId(1),
                low: 0.,
                high: 1.,
                ramp_frames: 0,
            })],
            sends: vec![],
            tail_frames: 0,
        }],
    ] {
        assert!(matches!(
            plan(vec![[1.; 2]], 1).with_buses(buses, vec![Some(0)]),
            Err(Error::InvalidInput)
        ));
    }
    assert!(
        plan(vec![[1.; 2]], 1)
            .with_buses(vec![], vec![Some(0)])
            .is_err()
    );
    assert!(plan(vec![[1.; 2]], 1).with_buses(vec![], vec![]).is_err());
}

#[test]
fn bus_tail_starts_after_the_last_voice_processor_frame_not_the_host_block() {
    for block in [1, 7, 64, 129] {
        let prepared = plan(vec![[1.; 2]], 1)
            .with_voice_chains(
                vec![VoiceChain::new(vec![], vec![filter()], 2).unwrap()],
                vec![Some(0)],
            )
            .unwrap()
            .with_buses(vec![filtered(3)], vec![Some(0)])
            .unwrap();
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        let mut audio = [[0.; 2]; 140];
        support::without_heap(|| {
            rt.trigger(input(1), 60, 1.).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (i, frame) in audio.iter().enumerate() {
                let expected = if i < 6 {
                    (0..=i.min(2)).map(|j| impulse(j) * impulse(i - j)).sum()
                } else {
                    0.
                };
                near(*frame, [expected; 2]);
            }
            assert_eq!(rt.voice_count(), 0);
        });
    }
}

#[test]
fn bus_mix_scales_a_bus_and_redirects_only_its_own_output() {
    let bus = |sends| Bus {
        processors: vec![],
        sends,
        tail_frames: 0,
    };
    // Bus 0 outputs to bus 1 and sends half to the main output; bus 1 is the parent.
    let prepared = plan(vec![[1., 1.]; 4], 1)
        .with_buses(
            vec![
                bus(vec![send(Some(1), 1.), send(None, 0.5)]),
                bus(vec![send(None, 1.)]),
            ],
            vec![Some(0)],
        )
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    assert_eq!(rt.bus_count(), 2);
    rt.set_bus_mix(
        0,
        BusMix {
            gain: [0.5, 0.25],
            output: Some(0),
        },
    )
    .unwrap();
    assert_eq!(
        rt.set_bus_mix(2, BusMix::default()),
        Err(Error::InvalidInput)
    );
    assert_eq!(
        rt.set_bus_mix(
            0,
            BusMix {
                gain: [f32::NAN, 1.],
                output: None
            }
        ),
        Err(Error::InvalidInput)
    );
    rt.trigger(input(1), 60, 1.).unwrap();
    let (mut main, mut out) = ([[0.; 2]; 2], [[0.; 2]; 2]);
    rt.render_split(&mut main, &mut [&mut out]).unwrap();
    // The redirected output skips bus 1; the aux send still reaches the main output.
    assert_eq!(out[0], [0.5, 0.25]);
    assert_eq!(main[0], [0.25, 0.125]);
    // Bus peaks are after the bus's own gain; bus 1 got nothing this time.
    let mut peaks = Vec::new();
    rt.take_bus_peaks(|bus, peak| peaks.push((bus, peak)));
    assert_eq!(peaks, [(0, [0.5, 0.25]), (1, [0.; 2])]);
    rt.take_bus_peaks(|_, peak| assert_eq!(peak, [0.; 2], "taking resets"));
    assert_eq!(
        rt.render_split(&mut main, &mut [&mut [[0.; 2]; 1]]),
        Err(Error::InvalidInput)
    );
    // An output beyond those given falls back to the bus's own target.
    rt.set_bus_mix(
        0,
        BusMix {
            gain: [1.; 2],
            output: Some(3),
        },
    )
    .unwrap();
    rt.render(&mut main).unwrap();
    assert_eq!(main[0], [1.5, 1.5]);
}

#[test]
fn a_bus_reverb_rings_after_the_note_and_ends_with_its_tail_without_heap_use() {
    let settings = ReverbSettings {
        decay_seconds: 0.5,
        size: 0.75,
        damping_hz: 6_000.,
        modulation_seconds: 0.0005,
        diffusion: 0.375,
        predelay_seconds: 0.,
        input_cutoff_hz: 20_000.,
        low_shelf_db: 0.,
        width: 1.,
    };
    let tail = settings.tail_frames(48000);
    let prepared = plan(vec![[0.5, 0.5]], 1)
        .with_buses(
            vec![Bus {
                processors: vec![Processor::Reverb(settings)],
                sends: vec![send(None, 1.)],
                tail_frames: tail,
            }],
            vec![Some(0)],
        )
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut audio = vec![[0.; 2]; 48_000];
    support::without_heap(|| {
        rt.trigger(input(1), 60, 1.).unwrap();
        for chunk in audio.chunks_mut(100) {
            rt.render(chunk).unwrap();
        }
    });
    let energy = |a: &[Frame]| a.iter().flatten().map(|x| x * x).sum::<f32>();
    assert!(audio.iter().flatten().all(|x| x.is_finite()));
    // The one-frame note is over by frame 1; the tail rings well past it.
    assert!(energy(&audio[2_000..12_000]) > 1e-6, "no tail");
    assert!(energy(&audio[40_000..]) < energy(&audio[2_000..12_000]) * 1e-3);
    // A reverb in a voice chain is refused.
    assert!(
        plan(vec![[1.; 2]], 1)
            .with_voice_chains(
                vec![VoiceChain::new(vec![Processor::Reverb(settings)], vec![], 0).unwrap()],
                vec![Some(0)]
            )
            .is_err()
    );
}

#[test]
fn a_bus_convolution_mixes_the_impulse_response_with_the_dry_signal_without_heap_use() {
    let mut h = vec![0.; 6_000];
    h[3] = 0.5;
    h[70] = -0.25;
    h[5_000] = 0.125;
    let impulse = Impulse::new(h.clone(), h.clone()).unwrap();
    let tail = 6_000 + 3 * 8_192;
    let render = |buses: Option<Vec<Bus>>| {
        let mut prepared = plan(vec![[0.5, 0.5]], 1);
        if let Some(buses) = buses {
            prepared = prepared
                .with_impulses(vec![impulse.clone()])
                .with_buses(buses, vec![Some(0)])
                .unwrap();
        }
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        let mut audio = vec![[0.; 2]; 12_000];
        support::without_heap(|| {
            rt.trigger(input(1), 60, 1.).unwrap();
            for chunk in audio.chunks_mut(100) {
                rt.render(chunk).unwrap();
            }
        });
        audio
    };
    let dry = render(None);
    let wet = render(Some(vec![Bus {
        processors: vec![Processor::Convolution {
            impulse: 0,
            dry: 0.5,
            wet: 2.,
        }],
        sends: vec![send(None, 1.)],
        tail_frames: tail as u32,
    }]));
    for channel in 0..2 {
        for n in 0..wet.len() {
            let mut expected = 0.5 * dry[n][channel];
            for (k, h) in h.iter().enumerate().filter(|(_, h)| **h != 0.) {
                if let Some(x) = n.checked_sub(k).map(|i| dry[i][channel]) {
                    expected += 2. * h * x;
                }
            }
            assert!(
                (wet[n][channel] - expected).abs() < 1e-5,
                "frame {n}: {} against {expected}",
                wet[n][channel]
            );
        }
    }
    assert!(wet[5_000][0].abs() > 0.01, "the late reflection is missing");
    // A convolution in a voice chain, or one with a missing impulse, is refused.
    assert!(
        plan(vec![[1.; 2]], 1)
            .with_voice_chains(
                vec![
                    VoiceChain::new(
                        vec![Processor::Convolution {
                            impulse: 0,
                            dry: 0.,
                            wet: 1.
                        }],
                        vec![],
                        0
                    )
                    .unwrap()
                ],
                vec![Some(0)]
            )
            .is_err()
    );
    assert!(
        plan(vec![[1.; 2]], 1)
            .with_buses(
                vec![Bus {
                    processors: vec![Processor::Convolution {
                        impulse: 0,
                        dry: 0.,
                        wet: 1.
                    }],
                    sends: vec![send(None, 1.)],
                    tail_frames: 0,
                }],
                vec![Some(0)]
            )
            .is_err()
    );
}

#[test]
fn a_bus_convolution_swaps_its_impulse_without_heap_use_and_rings_for_the_new_tail() {
    let mut first = vec![0.; 4];
    first[0] = 1.;
    let first = Impulse::new(first.clone(), first).unwrap();
    let prepared = plan(vec![[0.5, 0.5]], 1)
        .with_impulses(vec![first])
        .with_buses(
            vec![Bus {
                processors: vec![Processor::Convolution { impulse: 0, dry: 0., wet: 1. }],
                sends: vec![send(None, 1.)],
                tail_frames: 100,
            }],
            vec![Some(0)],
        )
        .unwrap();
    assert_eq!(prepared.convolution_slots(), 1);
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut audio = vec![[0.; 2]; 20_000];
    rt.trigger(input(1), 60, 1.).unwrap();
    rt.render(&mut audio).unwrap();
    assert!((audio[0][0] - 0.5).abs() < 1e-6);
    // A later reflection than the plan's tail: built on the control side.
    let mut h = vec![0.; 16_000];
    h[10] = 0.5;
    h[15_000] = 0.25;
    let second = Impulse::new(h.clone(), h).unwrap();
    let mut upload = ConvolutionUpload::new(&second, 0., 1.);
    assert!(rt.swap_convolution(1, &mut upload).is_err());
    support::without_heap(|| rt.swap_convolution(0, &mut upload).unwrap());
    rt.trigger(input(2), 60, 1.).unwrap();
    rt.render(&mut audio).unwrap();
    assert!((audio[10][0] - 0.25).abs() < 1e-5, "{}", audio[10][0]);
    assert!(audio[0][0].abs() < 1e-6, "the old impulse is gone");
    assert!((audio[15_000][0] - 0.125).abs() < 1e-5, "the new tail rings: {}", audio[15_000][0]);
}
