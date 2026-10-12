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
fn mix_block_blends_bypasses_and_ramps_by_slot_controls() {
    let range = |kind: SlotKind| ControlRange {
        control: slot_control(kind, -1, 3, 1),
        low: 0.,
        high: kind.max(),
        ramp_frames: 4,
    };
    let definition = |kind: SlotKind, value| ControlDefinition {
        id: slot_control(kind, -1, 3, 1),
        domain: ControlDomain::Real {
            min: 0.,
            max: kind.max(),
        },
        default: ControlValue::Real(value),
    };
    for block in [1, 7, 64] {
        let prepared = plan(vec![[1.; 2]; 400], 1)
            .with_controls(vec![
                definition(SlotKind::Dry, 0.),
                definition(SlotKind::Output, 1.),
                definition(SlotKind::Bypass, 0.),
            ])
            .unwrap()
            .with_buses(
                vec![Bus {
                    processors: vec![
                        Processor::Mix {
                            count: 1,
                            dry: range(SlotKind::Dry),
                            wet: range(SlotKind::Output),
                            bypass: range(SlotKind::Bypass),
                        },
                        Processor::Gain(3.),
                    ],
                    sends: vec![send(None, 1.)],
                    tail_frames: 0,
                }],
                vec![Some(0)],
            )
            .unwrap();
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        let plan = rt.active_plan();
        rt.trigger(input(1), 60, 1.).unwrap();
        let run = |rt: &mut Runtime, frames: usize| {
            let mut audio = vec![[0.; 2]; frames];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            audio[frames - 1][0]
        };
        let set = |rt: &mut Runtime, kind: SlotKind, value| {
            rt.edit_controls(
                plan,
                None,
                &[ControlWrite {
                    id: slot_control(kind, -1, 3, 1),
                    value: ControlValue::Real(value),
                }],
            )
            .unwrap();
        };
        assert!((run(&mut rt, 20) - 3.).abs() < 1e-9, "wet only");
        set(&mut rt, SlotKind::Dry, 0.5);
        assert!((run(&mut rt, 20) - 3.5).abs() < 1e-9, "dry joins");
        set(&mut rt, SlotKind::Bypass, 1.);
        assert!(
            (run(&mut rt, 20) - 1.).abs() < 1e-9,
            "bypass is dry at unity"
        );
        set(&mut rt, SlotKind::Bypass, 0.);
        set(&mut rt, SlotKind::Output, 0.);
        assert!((run(&mut rt, 20) - 0.5).abs() < 1e-9, "silent wet");
    }
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
                processors: vec![Processor::Convolution {
                    impulse: 0,
                    dry: 0.,
                    wet: 1.,
                }],
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
    assert!(
        (audio[15_000][0] - 0.125).abs() < 1e-5,
        "the new tail rings: {}",
        audio[15_000][0]
    );
}

#[test]
fn a_voice_chain_mix_block_follows_slot_controls_per_voice() {
    let range = |kind: SlotKind| ControlRange {
        control: slot_control(kind, 7, 2, -1),
        low: 0.,
        high: kind.max(),
        ramp_frames: 4,
    };
    let definition = |kind: SlotKind, value| ControlDefinition {
        id: slot_control(kind, 7, 2, -1),
        domain: ControlDomain::Real {
            min: 0.,
            max: kind.max(),
        },
        default: ControlValue::Real(value),
    };
    // One voice and two (the second makes a batch of lanes).
    for (voices, block) in [(1, 64), (2, 7), (3, 1)] {
        let prepared = plan(vec![[1.; 2]; 400], 1)
            .with_controls(vec![
                definition(SlotKind::Dry, 0.),
                definition(SlotKind::Output, 1.),
                definition(SlotKind::Bypass, 0.),
            ])
            .unwrap()
            .with_voice_chains(
                vec![
                    VoiceChain::new(
                        vec![
                            Processor::Mix {
                                count: 1,
                                dry: range(SlotKind::Dry),
                                wet: range(SlotKind::Output),
                                bypass: range(SlotKind::Bypass),
                            },
                            Processor::Gain(3.),
                        ],
                        vec![],
                        0,
                    )
                    .unwrap(),
                ],
                vec![Some(0)],
            )
            .unwrap();
        let mut rt = Runtime::new(prepared, limits()).unwrap();
        let plan = rt.active_plan();
        for id in 0..voices {
            rt.trigger(input(id + 1), 60, 1.).unwrap();
        }
        let n = voices as f32;
        let run = |rt: &mut Runtime, frames: usize| {
            let mut audio = vec![[0.; 2]; frames];
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            audio[frames - 1][0]
        };
        let set = |rt: &mut Runtime, kind: SlotKind, value| {
            rt.edit_controls(
                plan,
                None,
                &[ControlWrite {
                    id: slot_control(kind, 7, 2, -1),
                    value: ControlValue::Real(value),
                }],
            )
            .unwrap();
        };
        assert!(
            (run(&mut rt, 20) - 3. * n).abs() < 1e-5,
            "wet only, {voices} voices"
        );
        set(&mut rt, SlotKind::Dry, 0.5);
        assert!((run(&mut rt, 20) - 3.5 * n).abs() < 1e-5, "dry joins");
        set(&mut rt, SlotKind::Bypass, 1.);
        assert!(
            (run(&mut rt, 20) - n).abs() < 1e-5,
            "bypass is dry at unity"
        );
        set(&mut rt, SlotKind::Bypass, 0.);
        set(&mut rt, SlotKind::Output, 0.);
        assert!((run(&mut rt, 20) - 0.5 * n).abs() < 1e-5, "silent wet");
    }
}

#[test]
fn voice_send_taps_preserve_amplifier_position_and_sum_before_return_dsp_without_heap() {
    const SEND: ControlId = ControlId(22);
    let bypass = slot_control(SlotKind::Bypass, 0, 0, -1);
    let address = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_SENDLEVEL_0").unwrap(),
        group: 0,
        slot: 0,
        generic: -1,
    };
    let bypass_address = EngineParameterAddress {
        parameter: engine_parameter_id("ENGINE_PAR_EFFECT_BYPASS").unwrap(),
        ..address
    };
    for block in [1, 7, 64, 129] {
        let prepared = Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[1.0; 2]; 512].into_boxed_slice()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.0,
                velocity_high: 1.0,
                gain: 0.5,
                envelope: Envelope::new(0, 0, 0, 0.25, 0).unwrap(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_controls(vec![
            ControlDefinition {
                id: SEND,
                domain: ControlDomain::Real { min: 0.0, max: 4.0 },
                default: ControlValue::Real(0.5),
            },
            ControlDefinition {
                id: bypass,
                domain: ControlDomain::Real { min: 0.0, max: 1.0 },
                default: ControlValue::Real(0.0),
            },
        ])
        .unwrap()
        .with_buses(
            vec![
                Bus {
                    processors: vec![Processor::Gain(2.0)],
                    sends: vec![send(None, 1.0)],
                    tail_frames: 0,
                },
                Bus {
                    processors: vec![Processor::Delay(
                        Delay::new(3, [[0.0; 2]; 2], 0.0, 1.0).unwrap(),
                    )],
                    sends: vec![send(None, 1.0)],
                    tail_frames: 3,
                },
            ],
            vec![None],
        )
        .unwrap()
        .with_voice_chains(
            vec![
                VoiceChain::new(
                    vec![Processor::Gain(2.0)],
                    vec![Processor::Gain(3.0), Processor::Gain(0.0)],
                    0,
                )
                .unwrap()
                .with_taps(vec![
                    VoiceSendTap {
                        position: VoiceSendPosition::BeforeAmplitude(1),
                        bus: 0,
                        gain: Parameter::Control(ControlRange {
                            control: SEND,
                            low: 0.0,
                            high: 4.0,
                            ramp_frames: 0,
                        }),
                        bypass: Parameter::Control(ControlRange {
                            control: bypass,
                            low: 0.0,
                            high: 1.0,
                            ramp_frames: 0,
                        }),
                    },
                    VoiceSendTap {
                        position: VoiceSendPosition::AfterAmplitude(1),
                        bus: 1,
                        gain: Parameter::Constant(0.5),
                        bypass: Parameter::Constant(0.0),
                    },
                ])
                .unwrap(),
            ],
            vec![Some(0)],
        )
        .unwrap()
        .with_engine_parameters(
            vec![EngineParameterBinding {
                address,
                control: SEND,
                law: EngineParameterLaw::CubicGain { unity: 396851.0 },
            }],
            vec![],
        )
        .unwrap();
        let mut runtime = Runtime::new(prepared, limits()).unwrap();
        let mut audio = [[0.0; 2]; 128];
        support::without_heap(|| {
            runtime.trigger(input(1), 60, 1.0).unwrap();
            runtime.trigger(input(2), 60, 1.0).unwrap();
            for chunk in audio.chunks_mut(block) {
                runtime.render(chunk).unwrap();
            }
        });
        for (i, sample) in audio.into_iter().enumerate() {
            near(sample, [if i < 3 { 4.0 } else { 4.75 }; 2]);
        }
        support::without_heap(|| {
            runtime.set_engine_parameter(address, 396851).unwrap();
            let mut next = [[0.0; 2]; 8];
            runtime.render(&mut next).unwrap();
            for sample in next {
                near(sample, [8.75; 2]);
            }
            runtime.set_engine_parameter(bypass_address, 1).unwrap();
            runtime.render(&mut next).unwrap();
            for sample in next {
                near(sample, [0.75; 2]);
            }
        });
    }
}

#[test]
fn voice_modulation_gain_is_at_the_amplifier_between_send_taps() {
    for block in [1, 7, 64, 129] {
        let prepared = plan(vec![[1.0; 2]; 256], 1)
            .with_buses(
                vec![
                    Bus {
                        processors: vec![],
                        sends: vec![send(None, 1.0)],
                        tail_frames: 0,
                    },
                    Bus {
                        processors: vec![],
                        sends: vec![send(None, 1.0)],
                        tail_frames: 0,
                    },
                ],
                vec![None],
            )
            .unwrap()
            .with_voice_chains(
                vec![
                    VoiceChain::new(
                        vec![Processor::Gain(2.0)],
                        vec![Processor::Gain(3.0), Processor::Gain(0.0)],
                        0,
                    )
                    .unwrap()
                    .with_taps(vec![
                        VoiceSendTap {
                            position: VoiceSendPosition::BeforeAmplitude(1),
                            bus: 0,
                            gain: Parameter::Constant(1.0),
                            bypass: Parameter::Constant(0.0),
                        },
                        VoiceSendTap {
                            position: VoiceSendPosition::AfterAmplitude(1),
                            bus: 1,
                            gain: Parameter::Constant(1.0),
                            bypass: Parameter::Constant(0.0),
                        },
                    ])
                    .unwrap(),
                ],
                vec![Some(0)],
            )
            .unwrap()
            .with_voice_modulation(
                vec![ModProgram {
                    breakpoints: vec![],
                    sources: vec![ModSource::Envelope(
                        Envelope::new(64, 0, 0, 1.0, 0).unwrap(),
                    )],
                    routes: vec![ModRoute::new(0, ModTarget::Attenuate, 1.0)],
                    shapes: vec![],
                }],
                vec![Some(0)],
                vec![0],
            )
            .unwrap();
        let mut runtime = Runtime::new(prepared, limits()).unwrap();
        let mut audio = [[0.0; 2]; 128];
        support::without_heap(|| {
            runtime.trigger(input(1), 60, 1.0).unwrap();
            for chunk in audio.chunks_mut(block) {
                runtime.render(chunk).unwrap();
            }
        });
        for (i, sample) in audio.into_iter().enumerate() {
            near(sample, [2.0 + 6.0 * ((i + 1) as f32 / 64.0).min(1.0); 2]);
        }
    }
}

#[test]
fn settled_mix_and_retargeted_ramps_match_the_blend_equation_without_heap() {
    let ids = [ControlId(400), ControlId(401), ControlId(402)];
    let range = |control| ControlRange {
        control,
        low: 0.,
        high: 1.,
        ramp_frames: 8,
    };
    for bus in [false, true] {
        for (voices, block) in [(1, 64), (3, 1), (3, 7), (3, 64)] {
            let stages = vec![
                Processor::Mix {
                    count: 1,
                    dry: range(ids[0]),
                    wet: range(ids[1]),
                    bypass: range(ids[2]),
                },
                Processor::Gain(3.),
            ];
            let prepared = plan(vec![[0.125, -0.25]; 256], 1)
                .with_controls(
                    ids.into_iter()
                        .zip([0., 1., 0.])
                        .map(|(id, value)| ControlDefinition {
                            id,
                            domain: ControlDomain::Real { min: 0., max: 1. },
                            default: ControlValue::Real(value),
                        })
                        .collect(),
                )
                .unwrap();
            let prepared = if bus {
                prepared
                    .with_buses(
                        vec![Bus {
                            processors: stages,
                            sends: vec![send(None, 1.)],
                            tail_frames: 0,
                        }],
                        vec![Some(0)],
                    )
                    .unwrap()
            } else {
                prepared
                    .with_voice_chains(
                        vec![VoiceChain::new(stages, vec![], 0).unwrap()],
                        vec![Some(0)],
                    )
                    .unwrap()
            };
            let mut rt = Runtime::new(prepared, limits()).unwrap();
            let owner = rt.active_plan();
            support::without_heap(|| {
                for id in 0..voices {
                    rt.trigger(input(id + 1), 60, 1.).unwrap();
                }
            });
            let mut from = [0., 1., 0.];
            for (phase, target, frames) in [
                (0, [0., 1., 0.], 20),
                (1, [0.5, 0.25, 0.5], 13),
                (2, [1., 0., 1.], 13),
                (3, [0., 1., 0.], 29),
            ] {
                let mut audio = vec![[0.; 2]; frames];
                support::without_heap(|| {
                    if phase != 0 {
                        for (id, value) in ids.into_iter().zip(target) {
                            rt.edit_controls(
                                owner,
                                None,
                                &[ControlWrite {
                                    id,
                                    value: ControlValue::Real(value),
                                }],
                            )
                            .unwrap();
                        }
                    }
                    for chunk in audio.chunks_mut(block) {
                        rt.render(chunk).unwrap();
                    }
                });
                for (i, actual) in audio.into_iter().enumerate() {
                    let [dry, wet, bypass] = std::array::from_fn::<_, 3, _>(|j| {
                        if phase == 0 || i >= 8 {
                            target[j]
                        } else {
                            from[j] + (target[j] - from[j]) * (i as f64 / 8.)
                        }
                    });
                    let expected = [0.125, -0.25].map(|x| {
                        let x = x * f64::from(voices);
                        ((dry * (1. - bypass) + bypass) * x + wet * (1. - bypass) * (3. * x)) as f32
                    });
                    assert_eq!(
                        actual, expected,
                        "bus {bus}, voices {voices}, block {block}, phase {phase}, frame {i}"
                    );
                }
                from = target;
            }
        }
    }
}
