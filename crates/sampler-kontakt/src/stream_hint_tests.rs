use super::*;

struct FloatSource {
    rate: u32,
    frames: usize,
    opens: Arc<AtomicU64>,
    cancel: Option<Arc<AtomicBool>>,
    fail: bool,
}

fn float_frame(i: usize) -> Frame {
    [
        f32::from_bits(0x3f000001 + i as u32 * 37),
        f32::from_bits(0x3f100003 + i as u32 * 19),
    ]
}

impl AssetSource for FloatSource {
    fn open(&self) -> io::Result<SampleReader> {
        self.opens.fetch_add(1, Ordering::Relaxed);
        let (cancel, fail) = (self.cancel.clone(), self.fail);
        Ok(SampleReader::custom(
            self.rate,
            self.frames,
            move |start, out| {
                if fail {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "authored read failure",
                    ));
                }
                for (i, frame) in out.iter_mut().enumerate() {
                    *frame = float_frame(start + i);
                }
                if let Some(cancel) = &cancel {
                    cancel.store(true, Ordering::Release);
                }
                Ok(())
            },
        ))
    }
}

fn sources(assets: &[Pcm]) -> HashMap<AssetId, Arc<dyn AssetSource>> {
    assets
        .iter()
        .map(|pcm| {
            (
                pcm.asset_id(),
                Arc::new(FloatSource {
                    rate: pcm.sample_rate(),
                    frames: pcm.frame_count(),
                    opens: Arc::new(AtomicU64::new(0)),
                    cancel: None,
                    fail: false,
                }) as Arc<dyn AssetSource>,
            )
        })
        .collect()
}

#[test]
fn hinted_preload_uses_exact_packed_cost_and_preserves_base_when_hints_do_not_fit() {
    for decoders in [1, 4] {
        for budget in [256, 511, 512] {
            let assets = vec![Pcm::streamed(48000, 4096).unwrap()];
            let (_, worker) = StreamCache::new(4).unwrap();
            let hints = HintFallback {
                head_frames: 32,
                ranges: vec![vec![745..777]],
                anchors: None,
            };
            let (streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
                sources(&assets),
                &assets,
                vec![vec![0..4096]],
                Some(vec![vec![0..32]]),
                Some(hints),
                None,
                worker,
                decoders,
                false,
                budget,
                HashSet::new(),
                true,
                &|| false,
            )
            .unwrap();
            assert_eq!(bytes, if budget >= 512 { 512 } else { 256 });
            assert!(streamer.physical_budget().status().peak_bytes <= budget);
            assert!(assets[0].resident_covers(0..32));
            assert_eq!(assets[0].resident_covers(745..777), budget >= 512);
            assert_eq!(
                streamer.hint_readiness(&assets).unwrap().complete(),
                budget >= 512
            );
            assert_eq!(
                streamer.selected_hint_head_frames(),
                (budget >= 512).then_some(32)
            );
            assert!(streamer.head_fallback().is_some());
            assert!(!streamer.head_readiness(&assets).complete());
            if budget >= 512 {
                assets[0].mark_cold();
                assert!(streamer.reload(&assets).is_err());
                assert!(
                    assets[0].resident_covers(745..777),
                    "failed expanded reload retains the admitted hinted generation"
                );
                assert_eq!(streamer.physical_budget().status().held_bytes, bytes);
            }
        }
    }
}

#[test]
fn hinted_minimum_failure_keeps_nonempty_base_anchors() {
    let assets = vec![Pcm::streamed(48000, 4096).unwrap()];
    let hinted = |_| {
        Ok(AttackAnchors {
            output_frames: 0,
            ranges: vec![vec![0..32, 745..777]],
            hint_ranges: Some(vec![vec![745..777]]),
        })
    };
    let base = |_| {
        Ok(AttackAnchors {
            output_frames: 0,
            ranges: vec![vec![0..32]],
            hint_ranges: None,
        })
    };
    let hints = HintFallback {
        head_frames: 128,
        ranges: vec![vec![745..873]],
        anchors: Some(&hinted),
    };
    let (_, worker) = StreamCache::new(4).unwrap();
    let (streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
        sources(&assets),
        &assets,
        vec![vec![0..4096]],
        Some(vec![vec![0..128]]),
        Some(hints),
        Some(&base),
        worker,
        4,
        false,
        400,
        HashSet::new(),
        true,
        &|| false,
    )
    .unwrap();
    assert_eq!(bytes, 256);
    assert_eq!(streamer.selected_anchor_head_frames(), Some(0));
    assert_eq!(streamer.selected_hint_head_frames(), None);
    assert!(assets[0].resident_covers(0..32));
    assert!(!assets[0].resident_covers(745..777));
    assert!(streamer.anchor_readiness(&assets).unwrap().complete());
    assert!(!streamer.onset_readiness(&assets).complete());
}

#[test]
fn hinted_shorter_anchor_reports_selected_union_separately_from_requested_coverage() {
    let assets = vec![Pcm::streamed(48000, 4096).unwrap()];
    let hinted = |_| {
        Ok(AttackAnchors {
            output_frames: 16,
            ranges: vec![vec![0..16, 745..777]],
            hint_ranges: Some(vec![vec![745..777]]),
        })
    };
    let hints = HintFallback {
        head_frames: 128,
        ranges: vec![vec![745..873]],
        anchors: Some(&hinted),
    };
    let (_, worker) = StreamCache::new(4).unwrap();
    let (streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
        sources(&assets),
        &assets,
        vec![vec![0..4096]],
        Some(vec![vec![0..128]]),
        Some(hints),
        None,
        worker,
        4,
        false,
        512,
        HashSet::new(),
        true,
        &|| false,
    )
    .unwrap();
    assert_eq!(bytes, 384);
    assert_eq!(streamer.selected_hint_head_frames(), Some(16));
    let requested = streamer.hint_readiness(&assets).unwrap();
    assert_eq!((requested.ranges, requested.source_frames), (1, 128));
    assert_eq!(
        (requested.covered_ranges, requested.covered_source_frames),
        (0, 32)
    );
    let selected = streamer.selected_hint_readiness(&assets).unwrap();
    assert_eq!((selected.ranges, selected.source_frames), (1, 32));
    assert_eq!(
        (selected.covered_ranges, selected.covered_source_frames),
        (1, 32)
    );
    assert!(streamer.anchor_readiness(&assets).unwrap().complete());
    assert!(!streamer.onset_readiness(&assets).complete());
}

#[test]
fn hinted_source_errors_and_cancellation_never_retry_or_publish() {
    for cancel_read in [false, true] {
        let cancel = Arc::new(AtomicBool::new(false));
        let pcm = Pcm::streamed(48000, 4096).unwrap();
        let opens = Arc::new(AtomicU64::new(0));
        let sources = HashMap::from([(
            pcm.asset_id(),
            Arc::new(FloatSource {
                rate: 48000,
                frames: 4096,
                opens: opens.clone(),
                cancel: cancel_read.then(|| cancel.clone()),
                fail: !cancel_read,
            }) as Arc<dyn AssetSource>,
        )]);
        let (_, worker) = StreamCache::new(4).unwrap();
        let hints = HintFallback {
            head_frames: 32,
            ranges: vec![vec![745..777]],
            anchors: None,
        };
        let error = Streamer::start_with_hint_fallbacks_cancelable(
            sources,
            std::slice::from_ref(&pcm),
            vec![vec![0..4096]],
            Some(vec![vec![0..32]]),
            Some(hints),
            None,
            worker,
            4,
            false,
            512,
            HashSet::new(),
            true,
            &|| cancel.load(Ordering::Acquire),
        )
        .err()
        .unwrap();
        assert_eq!(
            error.kind(),
            if cancel_read {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::InvalidData
            }
        );
        assert_eq!(
            opens.load(Ordering::Relaxed),
            1,
            "only the first quota-admissible candidate may open a source"
        );
        assert_eq!(pcm.head_bytes(), 0, "no staged prefix publishes on failure");
    }
}

#[test]
fn hinted_admission_deduplicates_aliases_and_preserves_required_and_pinned_sources() {
    for budget in [1151, 1152] {
        let pcm = Pcm::streamed(48000, 4096).unwrap();
        let other = Pcm::streamed(48000, 4096).unwrap();
        let pinned = Pcm::streamed(48000, 4096).unwrap();
        let full = Pcm::pending_resident(48000, 16).unwrap();
        let assets = vec![
            pcm.clone(),
            pcm.clone(),
            other.clone(),
            pinned.clone(),
            full.clone(),
        ];
        let (_, worker) = StreamCache::new(4).unwrap();
        let hints = HintFallback {
            head_frames: 16,
            ranges: vec![
                vec![745..761],
                vec![753..777],
                vec![745..777],
                vec![],
                vec![],
            ],
            anchors: None,
        };
        let result = Streamer::start_with_hint_fallbacks_cancelable(
            sources(&assets),
            &assets,
            vec![
                vec![0..4096],
                vec![0..4096],
                vec![0..4096],
                vec![0..32],
                vec![0..16],
            ],
            Some(vec![
                vec![0..8],
                vec![4..16],
                vec![0..16],
                vec![0..4],
                vec![0..16],
            ]),
            Some(hints),
            None,
            worker,
            4,
            false,
            budget,
            HashSet::from([pinned.asset_id()]),
            true,
            &|| false,
        )
        .unwrap();
        let (streamer, bytes) = result;
        // Alias union: 16+32 frames, distinct other:16+32, pinned32, required16.
        assert_eq!(bytes, if budget >= 1152 { 1152 } else { 640 });
        assert_eq!(streamer.hint_readiness(&assets).unwrap().source_frames, 64);
        assert_eq!(
            streamer.hint_readiness(&assets).unwrap().complete(),
            budget >= 1152
        );
        assert!(pinned.resident_covers(0..32));
        assert_eq!(full.resident_frames().unwrap().len(), 16);
        assert!(streamer.physical_budget().status().peak_bytes <= budget);
    }
}

#[test]
fn hinted_coverage_reports_partial_intersections_without_counting_aliases_twice() {
    let pcm = Pcm::streamed(48000, 4096).unwrap();
    let assets = vec![pcm.clone(), pcm.clone()];
    let (_, worker) = StreamCache::new(4).unwrap();
    let hints = HintFallback {
        head_frames: 32,
        ranges: vec![vec![745..777]; 2],
        anchors: None,
    };
    let (streamer, _) = Streamer::start_with_hint_fallbacks_cancelable(
        sources(&assets),
        &assets,
        vec![vec![0..4096]; 2],
        Some(vec![vec![0..32]; 2]),
        Some(hints),
        None,
        worker,
        1,
        false,
        1024,
        HashSet::new(),
        true,
        &|| false,
    )
    .unwrap();
    let retired = pcm
        .set_ranges(vec![(750, vec![[0.25; 2]; 8].into())])
        .unwrap();
    let readiness = streamer.hint_readiness(&assets).unwrap();
    assert_eq!(
        (readiness.assets, readiness.ranges, readiness.source_frames),
        (1, 1, 32)
    );
    assert_eq!(
        (readiness.covered_ranges, readiness.covered_source_frames),
        (0, 8)
    );
    drop(retired);
}

#[test]
fn hinted_alias_view_order_keeps_physical_recipe_and_distinct_forward_reverse_starts() {
    use sampler_core as core;
    for swapped in [false, true] {
        let pcm = Pcm::streamed(44100, 32768).unwrap();
        let assets = vec![pcm.clone(), pcm.clone()];
        let views = if swapped {
            [(20000, 32000, true), (1000, 12000, false)]
        } else {
            [(1000, 12000, false), (20000, 32000, true)]
        };
        let make = |assets: Vec<Pcm>| {
            let regions = views
                .iter()
                .enumerate()
                .map(|(i, &(start, end, reverse))| core::Region {
                    sample: i,
                    key_low: 60 + i as u8,
                    key_high: 60 + i as u8,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: core::Envelope::default(),
                    playback: core::Playback {
                        start,
                        end: Some(end),
                        transpose_semitones: 12. * 1.78125_f64.log2(),
                        direction: if reverse {
                            core::Direction::Reverse
                        } else {
                            core::Direction::Forward
                        },
                        ..Default::default()
                    },
                })
                .collect();
            core::Prepared::new(48000, assets, regions, 2)
                .unwrap()
                .with_velocity_curves(vec![core::VelocityCurve::Constant; 2])
                .unwrap()
                .with_voice_modulation(
                    vec![core::ModProgram {
                        sources: vec![core::ModSource::Constant],
                        routes: vec![core::ModRoute::new(
                            0,
                            core::ModTarget::SampleStart,
                            0.0041315858,
                        )],
                        ..Default::default()
                    }],
                    vec![Some(0); 2],
                    vec![180479; 2],
                )
                .unwrap()
        };
        let mut instrument = ir::Instrument::default();
        instrument.zones = views
            .iter()
            .enumerate()
            .map(|(i, &(start, end, reverse))| {
                let mut zone = ir::Zone::new(ir::AssetRef(i));
                zone.keys = ir::KeyRange {
                    low: 60 + i as u8,
                    high: 60 + i as u8,
                };
                zone.pitch = ir::KeyTracking::Fixed;
                zone.tune = ir::Pitch::Ratio(1.78125);
                zone.playback = ir::Playback {
                    start: start as u64,
                    end: Some(end as u64),
                    reverse,
                    start_range: 180479,
                    ..Default::default()
                };
                zone.routes.push(ir::RouteRef(0));
                zone
            })
            .collect();
        instrument.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Constant,
        });
        instrument.routes.push(ir::Route::new(
            ir::ModulatorRef(0),
            ir::Target::SampleStart,
            ir::Depth::Normalized(0.0041315858),
        ));
        let proof = make(assets.clone());
        let policy = StreamPolicy {
            headroom: 0.,
            max_step: 16.,
            ..Default::default()
        };
        let recipe = ConstantStartHints::new(&instrument, &proof, &assets, &|| false).unwrap();
        let base = start_ranges_with_offsets(&instrument, &assets, 48000, 128, &policy, false);
        let hints = recipe
            .ranges(&instrument, &assets, 48000, 128, &policy, &|| false)
            .unwrap();
        let mut union = physical_start_ranges(&assets, &base);
        union
            .get_mut(&pcm.asset_id())
            .unwrap()
            .extend(hints.iter().flatten().cloned());
        merge_start_ranges(union.get_mut(&pcm.asset_id()).unwrap());
        let budget = union[&pcm.asset_id()].iter().map(|r| r.len() * 8).sum();
        let opens = Arc::new(AtomicU64::new(0));
        let source = Arc::new(FloatSource {
            rate: 44100,
            frames: 32768,
            opens: opens.clone(),
            cancel: None,
            fail: false,
        }) as Arc<dyn AssetSource>;
        let (_, worker) = StreamCache::new(4).unwrap();
        let (streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
            HashMap::from([(pcm.asset_id(), source)]),
            &assets,
            start_ranges(&instrument, &assets, 48000, 128, &policy),
            Some(base),
            Some(HintFallback {
                head_frames: 128,
                ranges: hints,
                anchors: None,
            }),
            None,
            worker,
            4,
            false,
            budget,
            HashSet::new(),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(bytes, budget);
        assert_eq!(
            opens.load(Ordering::Relaxed),
            1,
            "one physical backend is prepared regardless of logical view order"
        );
        let readiness = streamer.selected_hint_readiness(&assets).unwrap();
        assert_eq!(readiness.assets, 1);
        assert_eq!(readiness.ranges, 2);
        assert!(readiness.complete());
        for key in [60, 61] {
            for block in [7, 128] {
                let golden = Pcm::new(44100, (0..32768).map(float_frame).collect()).unwrap();
                let resident = make(vec![golden.clone(), golden]);
                let limits = core::Limits::for_plan(&resident, 4, 4);
                let mut expected = core::Runtime::new(resident, limits).unwrap();
                let (cache, _worker) = StreamCache::new(4).unwrap();
                let mut actual = core::Runtime::new(make(assets.clone()), limits)
                    .unwrap()
                    .with_stream_cache(cache);
                actual.set_cold_starts(true);
                let input = core::Input {
                    protocol: core::Protocol::Clap,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key,
                    external_id: Some(1),
                };
                expected.trigger(input, key, 1.).unwrap();
                actual.trigger(input, key, 1.).unwrap();
                assert_eq!(actual.voice_count(), 1);
                let mut a = [[0.; 2]; 128];
                let mut b = a;
                for (a, b) in a.chunks_mut(block).zip(b.chunks_mut(block)) {
                    expected.render(a).unwrap();
                    actual.render(b).unwrap();
                }
                assert_eq!(
                    a.map(|f| f.map(f32::to_bits)),
                    b.map(|f| f.map(f32::to_bits)),
                    "swapped={swapped} key={key} block={block}"
                );
                assert_eq!(actual.stats().cold_starts, 0);
                assert_eq!(actual.stream_underruns(), 0);
            }
        }
    }
}

#[test]
fn hinted_static_offset_745_matches_resident_pcm_under_reverse_pitch_and_host_partitions() {
    use sampler_core::{
        Direction, Envelope, Input, Limits, ModProgram, ModRoute, ModSource, ModTarget, Playback,
        Prepared, Protocol, Region, Runtime, VelocityCurve,
    };
    for reverse in [false, true] {
        for step in [1_f64, 1.78125, 16.] {
            let make = |pcm| {
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
                        playback: Playback {
                            start: 1000,
                            end: Some(32000),
                            transpose_semitones: 12. * step.log2(),
                            direction: if reverse {
                                Direction::Reverse
                            } else {
                                Direction::Forward
                            },
                            ..Default::default()
                        },
                    }],
                    1,
                )
                .unwrap()
                .with_velocity_curves(vec![VelocityCurve::Constant])
                .unwrap()
                .with_voice_modulation(
                    vec![ModProgram {
                        sources: vec![ModSource::Constant],
                        routes: vec![ModRoute::new(0, ModTarget::SampleStart, 0.0041315858)],
                        ..Default::default()
                    }],
                    vec![Some(0)],
                    vec![180479],
                )
                .unwrap()
            };
            let mut instrument = ir::Instrument::default();
            let mut zone = ir::Zone::new(ir::AssetRef(0));
            zone.keys = ir::KeyRange { low: 60, high: 60 };
            zone.pitch = ir::KeyTracking::Fixed;
            zone.tune = ir::Pitch::Ratio(step);
            zone.playback = ir::Playback {
                start: 1000,
                end: Some(32000),
                reverse,
                start_range: 180479,
                ..Default::default()
            };
            zone.routes.push(ir::RouteRef(0));
            instrument.zones.push(zone);
            instrument.modulators.push(ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Constant,
            });
            instrument.routes.push(ir::Route::new(
                ir::ModulatorRef(0),
                ir::Target::SampleStart,
                ir::Depth::Normalized(0.0041315858),
            ));
            let assets = vec![Pcm::streamed(48000, 32768).unwrap()];
            let proof = make(assets[0].clone());
            assert_eq!(proof.constant_start_offset(0), Some(745));
            let policy = StreamPolicy {
                headroom: 0.,
                max_step: 16.,
                ..Default::default()
            };
            let recipe = ConstantStartHints::new(&instrument, &proof, &assets, &|| false).unwrap();
            let base = start_ranges_with_offsets(&instrument, &assets, 48000, 128, &policy, false);
            let hints = recipe
                .ranges(&instrument, &assets, 48000, 128, &policy, &|| false)
                .unwrap();
            let mut union = base[0].clone();
            union.extend(hints[0].clone());
            merge_start_ranges(&mut union);
            let budget = union.iter().map(|r| r.len() * 8).sum();
            let (_, worker) = StreamCache::new(4).unwrap();
            let (streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
                sources(&assets),
                &assets,
                start_ranges(&instrument, &assets, 48000, 128, &policy),
                Some(base),
                Some(HintFallback {
                    head_frames: 128,
                    ranges: hints.clone(),
                    anchors: None,
                }),
                None,
                worker,
                4,
                false,
                budget,
                HashSet::new(),
                true,
                &|| false,
            )
            .unwrap();
            assert_eq!(bytes, budget);
            assert_eq!(streamer.selected_hint_head_frames(), Some(128));
            assert!(
                streamer
                    .selected_hint_readiness(&assets)
                    .unwrap()
                    .complete()
            );
            assert!(assets[0].resident_covers(hints[0][0].clone()));
            for block in [1, 7, 64, 128] {
                let golden = Pcm::new(48000, (0..32768).map(float_frame).collect()).unwrap();
                let resident = make(golden);
                let limits = Limits::for_plan(&resident, 4, 4);
                let mut expected = Runtime::new(resident, limits).unwrap();
                let (cache, _worker) = StreamCache::new(4).unwrap();
                let mut actual = Runtime::new(make(assets[0].clone()), limits)
                    .unwrap()
                    .with_stream_cache(cache);
                actual.set_cold_starts(true);
                let input = Input {
                    protocol: Protocol::Clap,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(1),
                };
                expected.trigger(input, 60, 1.).unwrap();
                actual.trigger(input, 60, 1.).unwrap();
                assert_eq!(actual.voice_count(), 1);
                let mut a = [[0.; 2]; 128];
                let mut b = a;
                for (a, b) in a.chunks_mut(block).zip(b.chunks_mut(block)) {
                    expected.render(a).unwrap();
                    actual.render(b).unwrap();
                }
                assert_eq!(
                    a.map(|f| f.map(f32::to_bits)),
                    b.map(|f| f.map(f32::to_bits)),
                    "reverse={reverse} step={step} block={block}"
                );
                assert!(b.iter().flatten().any(|v| v.abs() > 0.01));
                assert_eq!(actual.stats().cold_starts, 0);
                assert_eq!(actual.stream_underruns(), 0);
            }
        }
    }
}
