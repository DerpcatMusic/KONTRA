use sampler_core::*;
mod support;

fn frame(i: usize) -> [f32; 2] {
    [i as f32 / 16384., -0.25]
}
fn plan(pcm: Pcm, playback: Playback, depth: f64, extra: Vec<ModRoute>) -> Prepared {
    let mut routes = vec![ModRoute {
        native: Some(ModNative {
            target: 5,
            flags: 16,
            source_type: 11,
        }),
        unipolar: true,
        ..ModRoute::new(0, ModTarget::SampleStart, depth)
    }];
    routes.extend(extra);
    Prepared::new(
        48000,
        vec![pcm],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback,
        }],
        1,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant])
    .unwrap()
    .with_voice_modulation(
        vec![ModProgram {
            sources: vec![ModSource::Constant, ModSource::Velocity],
            routes,
            ..Default::default()
        }],
        vec![Some(0)],
        vec![180479],
    )
    .unwrap()
}
fn playback(shape: LoopShape) -> Playback {
    Playback {
        start: 37,
        end: Some(20000),
        loop_range: Some(Loop {
            start: 10000,
            end: 18000,
            mode: LoopMode::Continuous,
            shape,
            passes: None,
        }),
        ..Default::default()
    }
}
fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}

#[test]
fn constant_prefix_uses_actual_linear_demand_and_matches_pcm_in_all_qualities() {
    for shape in [
        LoopShape::Wrap,
        LoopShape::PingPong,
        LoopShape::Crossfade { frames: 64 },
        LoopShape::EqualPowerCrossfade { frames: 64 },
    ] {
        for rate in [44100, 48000] {
            for step in [0.5_f64, 1., 1.78125, 16.] {
                let mut playback = playback(shape);
                playback.transpose_semitones = 12. * step.log2();
                let pcm = Pcm::streamed(rate, 24000).unwrap();
                let prepared = plan(pcm.clone(), playback, 0.0041315858, vec![]);
                let prefix = prepared.constant_start_forward_prefix(0).unwrap();
                assert_eq!(prefix.asset_id(), pcm.asset_id());
                let bound = step * f64::from(rate) / 48000.;
                let window =
                    support::allocated_bytes(|| assert!(prefix.window(128, bound).is_some()));
                assert_eq!(window, 0);
                let window = prefix.window(128, bound).unwrap();
                pcm.set_ranges(vec![(window.start, window.clone().map(frame).collect())])
                    .unwrap();
                for quality in [ResampleQuality::Realtime, ResampleQuality::High] {
                    for block in [1, 7, 64, 128] {
                        let resident = plan(
                            Pcm::new(rate, (0..24000).map(frame).collect()).unwrap(),
                            playback,
                            0.0041315858,
                            vec![],
                        );
                        let limits = Limits::for_plan(&resident, 4, 4);
                        let mut expected = Runtime::new(resident, limits).unwrap();
                        let (cache, _worker) = StreamCache::new(4).unwrap();
                        let mut actual =
                            Runtime::new(plan(pcm.clone(), playback, 0.0041315858, vec![]), limits)
                                .unwrap()
                                .with_stream_cache(cache);
                        expected.set_resample_quality(quality);
                        actual.set_resample_quality(quality);
                        actual.set_cold_starts(true);
                        let mut a = [[0.; 2]; 128];
                        let mut b = a;
                        support::without_heap(|| {
                            expected.trigger(input(), 60, 1.).unwrap();
                            actual.trigger(input(), 60, 1.).unwrap();
                            for (a, b) in a.chunks_mut(block).zip(b.chunks_mut(block)) {
                                expected.render(a).unwrap();
                                actual.render(b).unwrap();
                            }
                        });
                        assert_eq!(
                            a.map(|f| f.map(f32::to_bits)),
                            b.map(|f| f.map(f32::to_bits)),
                            "rate={rate} step={step} block={block} shape={shape:?} quality={quality:?}"
                        );
                        assert!(b.iter().flatten().any(|v| v.abs() > 0.01));
                        assert_eq!(actual.stats().cold_starts, 0);
                        assert_eq!(actual.stream_underruns(), 0);
                    }
                }
            }
        }
    }
}

#[test]
fn constant_prefix_strictly_rejects_transition_crossing_reverse_controls_and_unknowns() {
    let make = |mut p: Playback, depth, routes: Vec<ModRoute>| {
        if routes
            .iter()
            .any(|r| matches!(r.target, ModTarget::LoopStart | ModTarget::LoopLength))
        {
            // Real admission requires an authored loop-control lane.
            p.start = 0;
            p.end = None;
            p.loop_control_crossfade = Some(0);
        }
        plan(Pcm::streamed(48000, 24000).unwrap(), p, depth, routes)
    };
    for shape in [
        LoopShape::Wrap,
        LoopShape::PingPong,
        LoopShape::Crossfade { frames: 64 },
        LoopShape::EqualPowerCrossfade { frames: 64 },
    ] {
        let p = make(playback(shape), 0.0041315858, vec![]);
        let prefix = p.constant_start_forward_prefix(0).unwrap();
        let fade = if matches!(
            shape,
            LoopShape::Crossfade { .. } | LoopShape::EqualPowerCrossfade { .. }
        ) {
            64
        } else {
            0
        };
        let last = 18000 - 37 - 745 - fade - 2;
        assert_eq!(prefix.window(last, 1.).unwrap().end, 18000 - fade - 1);
        assert!(
            prefix.window(last + 1, 1.).is_none(),
            "touching transition must reject"
        );
        for step in [f64::NAN, f64::INFINITY, 0., 17.] {
            assert!(prefix.window(128, step).is_none());
        }
        assert!(prefix.window(usize::MAX, 1.).is_none());
    }
    let mut reversed = playback(LoopShape::Wrap);
    reversed.direction = Direction::Reverse;
    assert!(
        make(reversed, 0.0041315858, vec![])
            .constant_start_forward_prefix(0)
            .is_none()
    );
    assert!(
        make(playback(LoopShape::Wrap), 0., vec![])
            .constant_start_forward_prefix(0)
            .is_none()
    );
    assert!(
        make(
            playback(LoopShape::Wrap),
            0.0041315858,
            vec![ModRoute::new(1, ModTarget::SampleStart, 0.)]
        )
        .constant_start_forward_prefix(0)
        .is_none()
    );
    for target in [ModTarget::LoopStart, ModTarget::LoopLength] {
        assert!(
            make(
                playback(LoopShape::Wrap),
                0.0041315858,
                vec![ModRoute::new(0, target, 0.)]
            )
            .constant_start_forward_prefix(0)
            .is_none()
        );
    }
    assert!(
        make(playback(LoopShape::Wrap), 0.11, vec![])
            .constant_start_forward_prefix(0)
            .is_none(),
        "offset already crossing loop is not a prefix proof"
    );
    assert!(
        make(playback(LoopShape::Wrap), 1., vec![])
            .constant_start_forward_prefix(0)
            .is_none(),
        "past-view is not readiness"
    );
    let mut complex = playback(LoopShape::Wrap);
    complex.loop_range = None;
    complex.loop_slots[0] = Some(LoopSlot {
        range: Loop {
            start: 10000,
            end: 18000,
            mode: LoopMode::Continuous,
            shape: LoopShape::Wrap,
            passes: None,
        },
        tuning: 0.75,
    });
    assert!(
        make(complex, 0.0041315858, vec![])
            .constant_start_forward_prefix(0)
            .is_none()
    );
}

#[test]
fn affected_eight_source_bound_numeric_geometries_have_safe_745_prefixes() {
    // Raw numeric receipt: harmonics-loop-metadata.json, original NKI SHA95801984...
    // This proves current prepared geometry, not a new native modulation law.
    for (frames, start, length) in [
        (480664, 106579, 131072),
        (527995, 89050, 200000),
        (527728, 121436, 216000),
        (431993, 123540, 135000),
        (527914, 120742, 216000),
        (527556, 133185, 196608),
        (479731, 94981, 218700),
        (479382, 99600, 156250),
    ] {
        let step = 2.00000262260437_f64;
        let playback = Playback {
            transpose_semitones: 12. * step.log2(),
            loop_slots: [
                Some(LoopSlot {
                    range: Loop {
                        start,
                        end: start + length,
                        mode: LoopMode::Continuous,
                        shape: LoopShape::Wrap,
                        passes: None,
                    },
                    tuning: 1.,
                }),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            ..Default::default()
        };
        let p = plan(
            Pcm::streamed(48000, frames).unwrap(),
            playback,
            0.0041315858,
            vec![],
        );
        assert_eq!(p.constant_start_offset(0), Some(745));
        let prefix = p.constant_start_forward_prefix(0).unwrap();
        let window = prefix.window(515, step * 2_f64.powf(2. / 12.)).unwrap();
        assert!(window.contains(&745));
        assert!(window.start <= 1516 && window.end >= 1755);
        assert!(
            window.end < start,
            "entire initial read lies even before loop entry"
        );
    }
}
