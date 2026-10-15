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
                assert!(
                    reload(
                        &assets,
                        &streamer.sources,
                        &streamer.ranges,
                        streamer.head_budget,
                        &streamer.head_mutation
                    )
                    .is_err()
                );
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
        let recipe =
            ConstantStartHints::new(&instrument, &proof, &assets, 48000, 128, &policy, &|| false)
                .unwrap();
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
            let recipe =
                ConstantStartHints::new(&instrument, &proof, &assets, 48000, 128, &policy, &|| {
                    false
                })
                .unwrap();
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

#[test]
fn hinted_restore_keeps_selected_physical_recipe_for_all_reload_consumers() {
    use sampler_core::{Envelope, Limits, Playback, Prepared, Region, Runtime, ScriptInstanceId};
    for consumer in 0..3 {
        let pcm = Pcm::streamed(48000, 4096).unwrap();
        let assets = vec![pcm.clone(), pcm.clone()];
        let (_, worker) = StreamCache::new(4).unwrap();
        let (mut streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
            sources(&assets),
            &assets,
            vec![vec![0..4096]; 2],
            Some(vec![vec![0..32]; 2]),
            Some(HintFallback {
                head_frames: 32,
                ranges: vec![vec![745..777]; 2],
                anchors: None,
            }),
            None,
            worker,
            1,
            false,
            512,
            HashSet::new(),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(bytes, 512);
        assert!(matches!(
            streamer.restore_ranges[&pcm.asset_id()],
            RestoreRange::Admitted(_)
        ));
        if consumer != 1 {
            // Isolate direct and KSP requests from the initial background scan.
            streamer.stop.store(true, Ordering::Relaxed);
            let reloader = streamer.reloader.take().unwrap();
            reloader.thread().unpark();
            reloader.join().unwrap();
            streamer.stop.store(false, Ordering::Relaxed);
        }
        let plan = Prepared::new(
            48000,
            vec![pcm.clone()],
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
                    start: 745,
                    ..Default::default()
                },
            }],
            1,
        )
        .unwrap()
        .with_groups(1, vec![Some(0)])
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap();
        let limits = Limits::for_plan(&plan, 4, 4);
        let (cache, _runtime_worker) = StreamCache::new(4).unwrap();
        let mut runtime = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
        runtime.set_cold_starts(true);
        let active = runtime.active_plan();
        let wait = |runtime: &mut Runtime, consumed| {
            let until = Instant::now() + Duration::from_secs(3);
            loop {
                runtime.render(&mut []).unwrap();
                let progress = runtime.residency_progress(active).unwrap();
                assert_eq!(progress.failed, 0);
                if progress.consumed >= consumed {
                    break;
                }
                assert!(Instant::now() < until, "residency restore timed out");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        if consumer == 2 {
            runtime
                .request_group_residency(active, 0, false, ScriptInstanceId(0))
                .unwrap();
            wait(&mut runtime, 1);
            assert_eq!(pcm.head_bytes(), 0, "final owner really purged the heads");
            runtime
                .request_group_residency(active, 0, true, ScriptInstanceId(0))
                .unwrap();
            wait(&mut runtime, 2);
        } else {
            assert_eq!(streamer.purge(&assets, 1), bytes);
            assert_eq!(pcm.head_bytes(), 0);
            pcm.mark_cold();
            if consumer == 0 {
                assert_eq!(streamer.reload(&assets).unwrap(), 1);
            } else {
                streamer.reloader.as_ref().unwrap().thread().unpark();
                let until = Instant::now() + Duration::from_secs(3);
                while !pcm.resident_covers(745..777) {
                    assert!(Instant::now() < until, "background restore timed out");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        assert!(pcm.resident_covers(0..32));
        assert!(pcm.resident_covers(745..777));
        assert!(!pcm.resident_covers(0..4096));
        assert_eq!(streamer.physical_budget().status().held_bytes, bytes);
        assert!(streamer.physical_budget().status().peak_bytes <= 512);
        let mut out = [[0.; 2]; 32];
        pcm.copy_resident_range(745, &mut out).unwrap();
        assert_eq!(
            out.map(|f| f.map(f32::to_bits)),
            std::array::from_fn::<_, 32, _>(|i| float_frame(745 + i).map(f32::to_bits))
        );
        let input = sampler_core::Input {
            protocol: sampler_core::Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(1),
        };
        runtime.trigger(input, 60, 1.).unwrap();
        assert_eq!(runtime.voice_count(), 1);
        let mut rendered = [[0.; 2]; 32];
        runtime.render(&mut rendered).unwrap();
        assert_eq!(
            rendered.map(|f| f.map(f32::to_bits)),
            out.map(|f| f.map(f32::to_bits))
        );
        assert_eq!(runtime.stats().cold_starts, 0);
        assert_eq!(runtime.stream_underruns(), 0);
    }
}

#[test]
fn hinted_deferred_restore_retains_original_request_instead_of_empty_optional_success() {
    for lazy in [true, false] {
        let assets = vec![Pcm::streamed(48000, 4096).unwrap()];
        let (_, worker) = StreamCache::new(4).unwrap();
        let (streamer, bytes) = Streamer::start_with_onsets_cancelable(
            sources(&assets),
            &assets,
            vec![vec![0..4096]],
            None,
            worker,
            1,
            lazy,
            256,
            HashSet::new(),
            true,
            &|| false,
        )
        .unwrap();
        assert_eq!(bytes, 0);
        assert!(matches!(
            streamer.restore_ranges[&assets[0].asset_id()],
            RestoreRange::DeferredRequested(_)
        ));
        assets[0].mark_cold();
        let error = streamer.reload(&assets).unwrap_err();
        assert!(
            error
                .get_ref()
                .is_some_and(|e| e.is::<HeadBudgetExceeded>())
        );
        assert_eq!(assets[0].head_bytes(), 0);
        assert_eq!(streamer.physical_budget().status().held_bytes, 0);
    }
}

#[test]
fn hinted_trim_deduplicates_aliases_before_comparing_physical_budget() {
    let first = Pcm::streamed(48000, 4096).unwrap();
    let second = Pcm::streamed(48000, 4096).unwrap();
    let assets = vec![first.clone(), first.clone(), second.clone()];
    let (_, worker) = StreamCache::new(4).unwrap();
    let (streamer, bytes) = Streamer::start(
        sources(&assets),
        &assets,
        vec![vec![0..32]; 3],
        worker,
        1,
        false,
        512,
        HashSet::new(),
    )
    .unwrap();
    assert_eq!(bytes, 512);
    assert_eq!(
        streamer.trim(&assets, bytes, 1),
        0,
        "two logical aliases must not force eviction below the actual physical limit"
    );
    assert!(first.resident_covers(0..32) && second.resident_covers(0..32));
    assert_eq!(streamer.trim(&assets, 256, 1), 256);
    assert_eq!(first.head_bytes() + second.head_bytes(), 256);
}

#[test]
fn hinted_loop_prefix_uses_prepared_geometry_and_keeps_crossing_windows_omitted() {
    use sampler_core as core;
    for step in [1_f64, 1.78125, 16.] {
        let make = |pcm| {
            core::Prepared::new(
                48000,
                vec![pcm],
                vec![core::Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Default::default(),
                    playback: core::Playback {
                        transpose_semitones: 12. * step.log2(),
                        loop_range: Some(core::Loop {
                            start: 10000,
                            end: 20000,
                            mode: core::LoopMode::Continuous,
                            shape: core::LoopShape::Wrap,
                            passes: None,
                        }),
                        ..Default::default()
                    },
                }],
                1,
            )
            .unwrap()
            .with_velocity_curves(vec![core::VelocityCurve::Constant])
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
                vec![Some(0)],
                vec![180479],
            )
            .unwrap()
        };
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.keys = ir::KeyRange { low: 60, high: 60 };
        zone.pitch = ir::KeyTracking::Fixed;
        zone.tune = ir::Pitch::Ratio(step);
        zone.playback.start_range = 180479;
        zone.playback.looping = ir::Looping::Continuous(ir::LoopRange {
            start: 10000,
            end: 20000,
            crossfade: ir::Span::Frames(0),
            alternating: false,
        });
        zone.routes.push(ir::RouteRef(0));
        let instrument = ir::Instrument {
            zones: vec![zone],
            modulators: vec![ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Constant,
            }],
            routes: vec![ir::Route::new(
                ir::ModulatorRef(0),
                ir::Target::SampleStart,
                ir::Depth::Normalized(0.0041315858),
            )],
            ..Default::default()
        };
        let assets = vec![Pcm::streamed(48000, 24000).unwrap()];
        let prepared = make(assets[0].clone());
        let policy = StreamPolicy {
            headroom: 0.,
            max_step: 16.,
            ..Default::default()
        };
        let recipe = ConstantStartHints::new(
            &instrument,
            &prepared,
            &assets,
            48000,
            128,
            &policy,
            &|| false,
        )
        .unwrap();
        assert_eq!(recipe.loop_prefix, 1);
        assert_eq!(recipe.loops, 0);
        let hints = recipe
            .ranges(&instrument, &assets, 48000, 128, &policy, &|| false)
            .unwrap();
        let base = start_ranges_with_offsets(&instrument, &assets, 48000, 128, &policy, false);
        let mut union = base[0].clone();
        union.extend(hints[0].iter().cloned());
        merge_start_ranges(&mut union);
        let budget = union.iter().map(|r| r.len() * 8).sum();
        let (_, worker) = StreamCache::new(4).unwrap();
        let (streamer, _) = Streamer::start_with_hint_fallbacks_cancelable(
            sources(&assets),
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
            1,
            false,
            budget,
            HashSet::new(),
            true,
            &|| false,
        )
        .unwrap();
        assert!(
            streamer
                .selected_hint_readiness(&assets)
                .unwrap()
                .complete()
        );
        for block in [1, 7, 64, 128] {
            let full = make(Pcm::new(48000, (0..24000).map(float_frame).collect()).unwrap());
            let limits = core::Limits::for_plan(&full, 4, 4);
            let mut expected = core::Runtime::new(full, limits).unwrap();
            let (cache, _worker) = StreamCache::new(4).unwrap();
            let mut actual = core::Runtime::new(make(assets[0].clone()), limits)
                .unwrap()
                .with_stream_cache(cache);
            actual.set_cold_starts(true);
            let input = core::Input {
                protocol: core::Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(1),
            };
            expected.trigger(input, 60, 1.).unwrap();
            actual.trigger(input, 60, 1.).unwrap();
            let (mut a, mut b) = ([[0.; 2]; 128], [[0.; 2]; 128]);
            for (a, b) in a.chunks_mut(block).zip(b.chunks_mut(block)) {
                expected.render(a).unwrap();
                actual.render(b).unwrap();
            }
            assert_eq!(
                a.map(|f| f.map(f32::to_bits)),
                b.map(|f| f.map(f32::to_bits))
            );
            assert_eq!(actual.stats().cold_starts, 0);
            assert_eq!(actual.stream_underruns(), 0);
        }
        let crossing = ConstantStartHints::new(
            &instrument,
            &prepared,
            &assets,
            48000,
            24000,
            &policy,
            &|| false,
        )
        .unwrap();
        assert_eq!(crossing.loop_prefix, 0);
        assert_eq!(crossing.loops, 1);
        let zero =
            ConstantStartHints::new(&instrument, &prepared, &assets, 48000, 0, &policy, &|| {
                false
            })
            .unwrap();
        assert_eq!(zero.loop_prefix, 1);
        assert!(
            zero.ranges(&instrument, &assets, 48000, 0, &policy, &|| false)
                .unwrap()[0][0]
                .contains(&745)
        );
        assert_eq!(
            zero.ranges(&instrument, &assets, 48000, 1, &policy, &|| false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut short = instrument.clone();
        short.zones[0].playback.end = Some(700);
        let silent =
            ConstantStartHints::new(&short, &prepared, &assets, 48000, 128, &policy, &|| false)
                .unwrap();
        assert_eq!(
            (silent.eligible, silent.loop_prefix, silent.silent),
            (0, 0, 1)
        );
        let wrong_assets = vec![Pcm::streamed(48000, 24000).unwrap()];
        assert_eq!(
            ConstantStartHints::new(
                &instrument,
                &prepared,
                &wrong_assets,
                48000,
                128,
                &policy,
                &|| false
            )
            .err()
            .unwrap()
            .kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[test]
fn hinted_empty_optional_source_defers_original_request_per_physical_asset() {
    let assets = vec![
        Pcm::streamed(48000, 4096).unwrap(),
        Pcm::streamed(48000, 4096).unwrap(),
        Pcm::pending_resident(48000, 64).unwrap(),
        Pcm::streamed(48000, 4096).unwrap(),
    ];
    let (_, worker) = StreamCache::new(4).unwrap();
    let (mut streamer, bytes) = Streamer::start_with_onsets_cancelable(
        sources(&assets),
        &assets,
        vec![vec![0..4096], vec![0..4096], vec![0..64], vec![0..32]],
        Some(vec![vec![0..32], vec![], vec![], vec![]]),
        worker,
        1,
        false,
        1024,
        HashSet::from([assets[3].asset_id()]),
        true,
        &|| false,
    )
    .unwrap();
    assert_eq!(bytes, 1024);
    for i in [2, 3] {
        assert!(matches!(
            streamer.restore_ranges[&assets[i].asset_id()],
            RestoreRange::Admitted(_)
        ));
    }
    assert_eq!(assets[2].resident_frames().unwrap().len(), 64);
    assert!(assets[3].resident_covers(0..32));
    assert!(matches!(
        streamer.restore_ranges[&assets[0].asset_id()],
        RestoreRange::Admitted(_)
    ));
    assert!(matches!(
        streamer.restore_ranges[&assets[1].asset_id()],
        RestoreRange::DeferredRequested(_)
    ));
    assert_eq!(
        streamer.restore_ranges[&assets[1].asset_id()].as_ref(),
        &[0..4096]
    );
    assert!(assets[0].resident_covers(0..32));
    assert_eq!(assets[1].head_bytes(), 0);
    streamer.stop.store(true, Ordering::Relaxed);
    let reloader = streamer.reloader.take().unwrap();
    reloader.thread().unpark();
    reloader.join().unwrap();
    assets[1].mark_cold();
    let error = streamer.reload(&assets).unwrap_err();
    assert!(
        error
            .get_ref()
            .is_some_and(|e| e.is::<HeadBudgetExceeded>())
    );
    assert!(assets[0].resident_covers(0..32));
    assert_eq!(assets[1].head_bytes(), 0);
    assert_eq!(streamer.head_budget_status().held_bytes, bytes);
}
