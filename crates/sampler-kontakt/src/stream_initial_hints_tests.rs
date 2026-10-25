//! Source-free regressions for root's coordinated build. No library assets.
use super::super::{
    AssetSource, AttackAnchors, Frame, HintFallback, SampleReader, StreamCache, Streamer,
    bounded_attack_anchors, bounded_attack_anchors_with_hints, start_ranges,
    start_ranges_with_offsets,
};
use super::*;
use sampler_core as core;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const RATE: u32 = 48_000;
const LENGTH: usize = 32_768;

fn frame(index: usize) -> Frame {
    // Distinguishable positive values, losslessly stored as binary32.
    [
        f32::from_bits(0x3e800001 + index as u32 * 37),
        f32::from_bits(0x3f000003 + index as u32 * 19),
    ]
}

struct NumericSource {
    cancel: Option<Arc<AtomicBool>>,
}
impl AssetSource for NumericSource {
    fn header(&self) -> Option<(u32, usize)> {
        Some((RATE, LENGTH))
    }
    fn open(&self) -> io::Result<SampleReader> {
        let cancel = self.cancel.clone();
        Ok(SampleReader::custom(RATE, LENGTH, move |start, output| {
            for (index, sample) in output.iter_mut().enumerate() {
                *sample = frame(start + index);
            }
            if let Some(cancel) = &cancel {
                cancel.store(true, Ordering::Release);
            }
            Ok(())
        }))
    }
}

fn instrument(
    reverse: bool,
    step: f64,
    start: u64,
    end: Option<u64>,
    range: u64,
) -> ir::Instrument {
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.keys = ir::KeyRange { low: 60, high: 60 };
    zone.pitch = ir::KeyTracking::Fixed;
    zone.tune = ir::Pitch::Ratio(step);
    zone.playback = ir::Playback {
        start,
        end,
        reverse,
        start_range: range,
        ..Default::default()
    };
    zone.routes.push(ir::RouteRef(0));
    ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("controller-start-numeric".into()),
            encoding: ir::Encoding::Unknown,
            residency: ir::AssetResidency::Streamable,
            root_key: None,
            loops: vec![],
        }],
        zones: vec![zone],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Controller(113),
        }],
        routes: vec![ir::Route::new(
            ir::ModulatorRef(0),
            ir::Target::SampleStart,
            ir::Depth::Normalized(1.),
        )],
        ..Default::default()
    }
}

fn plan(instrument: &ir::Instrument, assets: Vec<Pcm>, cc: u8) -> core::Prepared {
    core::lower::lower(instrument, RATE, assets, |_, plan| Ok(plan))
        .unwrap()
        .with_initial_controllers(&[(113, cc)])
}

fn policy(budget: usize) -> StreamPolicy {
    StreamPolicy {
        head_budget: budget,
        headroom: 0.,
        max_step: 16.,
        resident_budget: None,
        block_frames: 64,
        slack: Duration::ZERO,
        voices: 2,
        decoders: 1,
        allow_cold_fallback: true,
        ..Default::default()
    }
}

fn preload(
    instrument: &ir::Instrument,
    prepared: &core::Prepared,
    assets: &[Pcm],
    policy: &StreamPolicy,
    head: usize,
    source_cancel: Option<Arc<AtomicBool>>,
    canceled: &(dyn Fn() -> bool + Sync),
) -> io::Result<(Streamer, StreamCache, usize)> {
    let recipe =
        InitialStartHints::new(instrument, prepared, assets, RATE, head, policy, canceled)?;
    let pins = HashSet::new();
    let anchors = |room| {
        bounded_attack_anchors(
            instrument,
            assets,
            RATE,
            head,
            &StreamPolicy {
                head_budget: room,
                ..*policy
            },
            &pins,
            canceled,
        )
    };
    let hinted = |room| {
        bounded_attack_anchors_with_hints(
            instrument,
            assets,
            RATE,
            head,
            &StreamPolicy {
                head_budget: room,
                ..*policy
            },
            &pins,
            Some(&recipe),
            canceled,
        )
    };
    let hints = (recipe.eligible != 0)
        .then(|| {
            recipe.ranges(head, canceled).map(|ranges| HintFallback {
                head_frames: head,
                ranges,
                anchors: Some(&hinted as &dyn Fn(usize) -> io::Result<AttackAnchors>),
            })
        })
        .transpose()?;
    let sources = assets
        .iter()
        .map(|pcm| {
            (
                pcm.asset_id(),
                Arc::new(NumericSource {
                    cancel: source_cancel.clone(),
                }) as Arc<dyn AssetSource>,
            )
        })
        .collect::<HashMap<_, _>>();
    let (cache, worker) = StreamCache::new(8).unwrap();
    let (streamer, bytes) = Streamer::start_with_hint_fallbacks_cancelable(
        sources,
        assets,
        start_ranges(instrument, assets, RATE, head, policy),
        Some(start_ranges_with_offsets(
            instrument, assets, RATE, head, policy, false,
        )),
        hints,
        Some(&anchors),
        worker,
        policy.decoders,
        false,
        policy.head_budget,
        HashSet::new(),
        true,
        canceled,
    )?;
    Ok((streamer, cache, bytes))
}

fn input() -> core::Input {
    core::Input {
        protocol: core::Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    }
}

fn runtime(plan: core::Prepared, cache: StreamCache) -> core::Runtime {
    let limits = core::Limits::for_plan(&plan, 4, 4);
    let mut runtime = core::Runtime::new(plan, limits)
        .unwrap()
        .with_stream_cache(cache);
    runtime.set_cold_starts(true);
    runtime
}

#[test]
fn initialized_start_hint_uses_actual_63_offset_and_final_bound_assignment() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let prepared = plan(&ir, assets.clone(), 63);
    assert_eq!(prepared.constant_start_offset(0), None);
    assert_eq!(prepared.initialized_start_offset(0), Some(5952));
    let recipe =
        InitialStartHints::new(&ir, &prepared, &assets, RATE, 64, &policy(4096), &|| false)
            .unwrap();
    assert_eq!(recipe.controller_zones(), 1);
    let ranges = recipe.ranges(64, &|| false).unwrap();
    assert!(ranges[0].iter().any(|range| range.contains(&5952)));
    assert!(!ranges[0].iter().any(|range| range.contains(&0)));
    assert_eq!(
        assets[0].head_bytes(),
        0,
        "geometry performs no PCM publication"
    );
    assert_eq!(
        plan(&ir, assets.clone(), 0)
            .with_initial_controllers(&[(113, 0), (113, 63)])
            .initialized_start_offset(0),
        Some(5952)
    );
    assert_eq!(
        plan(&ir, assets, 63)
            .with_initial_level(113, 1.)
            .initialized_start_offset(0),
        Some(12_000)
    );
    assert_eq!(prepared.initialized_start_offset(1), None);
}

#[test]
fn initialized_start_hint_loader_uses_real_script_init_binding() {
    let mut ir = instrument(false, 1., 0, None, 12_000);
    ir.behaviors.push(ir::Behavior {
        name: "numeric controller initialization".into(),
        language: ir::Language::Ksp,
        source: "on init\nset_controller(113, 63)\nend on\n".into(),
        slot: Some(0),
        state: vec![],
        requires: vec![],
    });
    // Exercise real source opening and KSP initialization without allowing
    // measured wall latency to choose this fixture's quota/horizon.
    let source: Arc<dyn AssetSource> = Arc::new(NumericSource { cancel: None });
    let policy = policy(8192);
    let mut opened = Streamer::open(
        vec![(source, std::path::Path::new("controller-start-numeric"))],
        RATE,
        &policy,
        0,
        &|| false,
        &mut |_, _| (),
    )
    .unwrap();
    opened.head = 64;
    opened.report.head_frames = 64;
    assert_eq!(opened.report.latency_samples, 0);
    let assets = opened.assets.clone();
    let loaded = crate::finish(
        ir,
        assets.clone(),
        vec!["controller-start-numeric".into()],
        &crate::Options {
            rate: RATE,
            ..Default::default()
        },
    )
    .unwrap();
    let streamed = super::super::Streamed::new(loaded, opened, assets, &|| false).unwrap();
    assert_eq!(streamed.loaded.plan.initialized_start_offset(0), Some(5952));
    assert!(
        streamed.streamer.head_fallback().is_some(),
        "full authored offset interval exceeds the cap"
    );
    assert!(
        streamed
            .streamer
            .selected_hint_readiness(&streamed.assets)
            .unwrap()
            .complete()
    );
    assert!(streamed.assets[0].resident_covers(5952..5953));
    assert!(!streamed.assets[0].resident_covers(12_000..12_001));
    assert!(streamed.streamer.head_budget_status().peak_bytes <= 8192);
    let mut actual = runtime(streamed.loaded.plan, streamed.cache);
    assert_eq!(
        actual
            .controller(actual.performance(0).unwrap(), 113)
            .unwrap(),
        (63_u64 * u64::from(u32::MAX) / 127) as u32
    );
    actual.trigger(input(), 60, 1.).unwrap();
    let mut output = [[0.; 2]; 32];
    actual.render(&mut output).unwrap();
    assert!(output.iter().flatten().any(|&value| value != 0.));
    assert_eq!(actual.stats().cold_starts, 0);
    assert_eq!(actual.stream_underruns(), 0);
}

#[test]
fn initialized_start_hint_changed_live_cc_uses_new_strict_demand() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let prepared = plan(&ir, assets.clone(), 63);
    let (streamer, cache, _) =
        preload(&ir, &prepared, &assets, &policy(4096), 64, None, &|| false).unwrap();
    assert!(
        streamer
            .selected_hint_readiness(&assets)
            .unwrap()
            .complete()
    );
    assert!(assets[0].resident_covers(5952..5953));
    assert!(!assets[0].resident_covers(12_000..12_001));
    let mut actual = runtime(prepared, cache);
    let performance = actual.performance(0).unwrap();
    actual.set_controller(performance, 113, u32::MAX).unwrap();
    actual.trigger(input(), 60, 1.).unwrap();
    let mut found = false;
    actual
        .visit_stream_demand(32, |demand| {
            assert_eq!(demand.key.asset, assets[0].asset_id());
            if demand.frames.contains(&12_000) {
                found = true;
                assert!(
                    !demand.resident,
                    "old initial windows cannot certify the live offset"
                );
            }
            assert!(!demand.frames.contains(&5952));
            true
        })
        .unwrap();
    assert!(found, "runtime recomputed CC113=127, offset 12000");
    let mut output = [[1.; 2]; 32];
    actual.render(&mut output).unwrap();
    assert_eq!(
        output, [[0.; 2]; 32],
        "strict cold hold; no shifted/stale onset"
    );
    assert_eq!(actual.stats().cold_starts, 1);
    assert_eq!(actual.controller(performance, 113).unwrap(), u32::MAX);
    assert!(
        streamer
            .selected_hint_readiness(&assets)
            .unwrap()
            .complete(),
        "this remains physical coverage of the old prediction, not current readiness"
    );
}

#[test]
fn initialized_start_hint_reverse_resampling_and_bounded_views_match_resident() {
    for reverse in [false, true] {
        for step in [1_f64, 1.78125, 16.] {
            for depth in [-0.5, 1., 3.] {
                let mut ir = instrument(reverse, step, 1000, Some(32_000), 12_000);
                ir.routes[0].depth = ir::Depth::Normalized(depth);
                let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
                let prepared = plan(&ir, assets.clone(), 63);
                let recipe = InitialStartHints::new(
                    &ir,
                    &prepared,
                    &assets,
                    RATE,
                    64,
                    &policy(65_536),
                    &|| false,
                )
                .unwrap();
                assert_eq!(
                    prepared.initialized_start_offset(0),
                    Some(match depth {
                        d if d < 0. => 0,
                        d if d > 1. => 12_000,
                        _ => 5952,
                    })
                );
                for range in recipe.ranges(64, &|| false).unwrap().iter().flatten() {
                    assert!(range.start >= 1000 && range.end <= 32_000 && !range.is_empty());
                }
                let (_streamer, cache, bytes) =
                    preload(&ir, &prepared, &assets, &policy(65_536), 64, None, &|| {
                        false
                    })
                    .unwrap();
                assert!(bytes <= 65_536);
                let resident = plan(
                    &ir,
                    vec![Pcm::new(RATE, (0..LENGTH).map(frame).collect()).unwrap()],
                    63,
                );
                let limits = core::Limits::for_plan(&resident, 4, 4);
                let mut expected = core::Runtime::new(resident, limits).unwrap();
                let mut actual = runtime(prepared, cache);
                expected.trigger(input(), 60, 1.).unwrap();
                actual.trigger(input(), 60, 1.).unwrap();
                let mut a = [[0.; 2]; 64];
                let mut b = a;
                for (a, b) in a.chunks_mut(7).zip(b.chunks_mut(7)) {
                    expected.render(a).unwrap();
                    actual.render(b).unwrap();
                }
                assert_eq!(
                    a.map(|f| f.map(f32::to_bits)),
                    b.map(|f| f.map(f32::to_bits)),
                    "reverse={reverse} step={step} depth={depth}"
                );
                assert!(b.iter().flatten().any(|&v| v != 0.));
                assert_eq!(actual.stats().cold_starts, 0);
                assert_eq!(actual.stream_underruns(), 0);
            }
        }
    }
    let ir = instrument(false, 1., 1000, Some(2000), 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let prepared = plan(&ir, assets.clone(), 63);
    let recipe =
        InitialStartHints::new(&ir, &prepared, &assets, RATE, 64, &policy(4096), &|| false)
            .unwrap();
    assert_eq!(
        recipe.eligible, 0,
        "offset beyond an authored view is silent"
    );
    assert!(
        recipe
            .ranges(64, &|| false)
            .unwrap()
            .iter()
            .all(Vec::is_empty)
    );
}

#[test]
fn initialized_start_hint_retains_authored_shape_signed_depth_and_intensity_laws() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let make = |assets| {
        let mut shaped = core::ModRoute::new(0, core::ModTarget::SampleStart, 0.8);
        shaped.invert = true;
        shaped.shape = Some(0);
        shaped.scale = Some(core::ModScale {
            source: 2,
            shape: Some(1),
            lag: 128,
            law: core::ModScaleLaw::Multiply,
        });
        shaped.more_scales[0] = Some(core::ModScale {
            source: 1,
            shape: None,
            lag: 0,
            law: core::ModScaleLaw::Multiply,
        });
        plan(&ir, assets, 63)
            .with_initial_controllers(&[(113, 63), (114, 99)])
            .with_voice_modulation(
                vec![core::ModProgram {
                    sources: vec![
                        core::ModSource::Controller(113),
                        core::ModSource::Constant,
                        core::ModSource::Controller(114),
                    ],
                    shapes: vec![
                        (0..128)
                            .map(|n| {
                                let x = n as f64 / 127.;
                                (x, x * x)
                            })
                            .collect(),
                        vec![(0., 0.1), (0.5, 0.8), (1., 1.)],
                    ],
                    routes: vec![
                        shaped,
                        core::ModRoute::new(1, core::ModTarget::SampleStart, -0.01),
                    ],
                    ..Default::default()
                }],
                vec![Some(0)],
                vec![12_000],
            )
            .unwrap()
    };
    let prepared = make(assets.clone());
    let offset = prepared.initialized_start_offset(0).unwrap();
    assert!(offset > 100 && offset < 12_000);
    let (_streamer, cache, _) =
        preload(&ir, &prepared, &assets, &policy(4096), 64, None, &|| false).unwrap();
    assert!(assets[0].resident_covers(offset as usize..offset as usize + 64));
    let resident = make(vec![
        Pcm::new(RATE, (0..LENGTH).map(frame).collect()).unwrap(),
    ]);
    let limits = core::Limits::for_plan(&resident, 4, 4);
    let mut expected = core::Runtime::new(resident, limits).unwrap();
    let mut actual = runtime(prepared, cache);
    expected.trigger(input(), 60, 1.).unwrap();
    actual.trigger(input(), 60, 1.).unwrap();
    let mut a = [[0.; 2]; 64];
    let mut b = a;
    expected.render(&mut a).unwrap();
    actual.render(&mut b).unwrap();
    assert_eq!(
        a.map(|f| f.map(f32::to_bits)),
        b.map(|f| f.map(f32::to_bits))
    );
}

#[test]
fn initialized_start_hint_rejects_unknown_inputs_even_in_zero_depth_scales_and_loops() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    for source in [
        core::ModSource::Velocity,
        core::ModSource::Key,
        core::ModSource::Random,
        core::ModSource::Pressure,
        core::ModSource::Timbre,
        core::ModSource::PitchBend,
        core::ModSource::Script(0),
        core::ModSource::ReleaseCounter { frames: 128 },
        core::ModSource::Envelope(Default::default()),
        core::ModSource::Controller(114),
    ] {
        for scale in [false, true] {
            let mut route = core::ModRoute::new(0, core::ModTarget::SampleStart, 0.);
            if scale {
                route.scale = Some(core::ModScale {
                    source: 1,
                    shape: None,
                    law: core::ModScaleLaw::Multiply,
                    lag: 0,
                });
            }
            let prepared = plan(&ir, assets.clone(), 63)
                .with_voice_modulation(
                    vec![core::ModProgram {
                        sources: if scale {
                            vec![core::ModSource::Constant, source]
                        } else {
                            vec![source]
                        },
                        routes: vec![route],
                        ..Default::default()
                    }],
                    vec![Some(0)],
                    vec![12_000],
                )
                .unwrap();
            assert_eq!(
                prepared.initialized_start_offset(0),
                None,
                "source={source:?} scale={scale}"
            );
            let recipe =
                InitialStartHints::new(&ir, &prepared, &assets, RATE, 64, &policy(4096), &|| false)
                    .unwrap();
            assert_eq!((recipe.eligible, recipe.unsupported), (0, 1));
        }
    }
    let mut looped = ir;
    looped.zones[0].playback.looping = ir::Looping::Continuous(ir::LoopRange {
        start: 10_000,
        end: 20_000,
        crossfade: ir::Span::Frames(0),
        alternating: false,
    });
    let prepared = plan(&looped, assets.clone(), 63);
    let recipe = InitialStartHints::new(
        &looped,
        &prepared,
        &assets,
        RATE,
        64,
        &policy(4096),
        &|| false,
    )
    .unwrap();
    assert_eq!((recipe.eligible, recipe.controller_loops), (0, 1));
}

#[test]
fn initialized_start_hint_budget_counts_physical_aliases_and_actual_guard_windows() {
    for budget in [1536, 3072, 8192] {
        let mut ir = instrument(false, 1.78125, 0, None, 12_000);
        ir.assets.push(ir.assets[0].clone());
        let mut alias = ir.zones[0].clone();
        alias.asset = Some(ir::AssetRef(1));
        alias.keys = ir::KeyRange { low: 61, high: 61 };
        ir.zones.push(alias);
        let pcm = Pcm::streamed(RATE, LENGTH).unwrap();
        let assets = vec![pcm.clone(), pcm.clone()];
        let prepared = plan(&ir, assets.clone(), 63);
        let (streamer, _cache, bytes) =
            preload(&ir, &prepared, &assets, &policy(budget), 128, None, &|| {
                false
            })
            .unwrap();
        assert!(bytes <= budget);
        assert_eq!(bytes, pcm.head_bytes(), "physical aliases are charged once");
        assert_eq!(streamer.head_budget_status().held_bytes, bytes);
        assert!(streamer.head_budget_status().peak_bytes <= budget);
        assert!(pcm.resident_covers(0..1), "base guards survive tiny caps");
        if let Some(head) = streamer.selected_hint_head_frames() {
            let recipe = InitialStartHints::new(
                &ir,
                &prepared,
                &assets,
                RATE,
                128,
                &policy(budget),
                &|| false,
            )
            .unwrap();
            let selected = recipe.ranges(head, &|| false).unwrap();
            assert_eq!(selected[0], selected[1]);
            assert!(
                selected[0]
                    .iter()
                    .all(|range| pcm.resident_covers(range.clone()))
            );
            assert_eq!(streamer.selected_hint_readiness(&assets).unwrap().assets, 1);
        }
    }
}

#[test]
fn initialized_start_hint_cancellation_leaves_no_partial_publication() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let prepared = plan(&ir, assets.clone(), 63);
    assert_eq!(
        InitialStartHints::new(&ir, &prepared, &assets, RATE, 64, &policy(4096), &|| true)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::Interrupted
    );
    let recipe =
        InitialStartHints::new(&ir, &prepared, &assets, RATE, 64, &policy(4096), &|| false)
            .unwrap();
    assert_eq!(
        recipe.ranges(64, &|| true).unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
    assert_eq!(
        recipe.ranges(65, &|| false).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    let canceled = Arc::new(AtomicBool::new(false));
    let result = preload(
        &ir,
        &prepared,
        &assets,
        &policy(4096),
        64,
        Some(canceled.clone()),
        &|| canceled.load(Ordering::Acquire),
    );
    assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
    assert!(
        canceled.load(Ordering::Acquire),
        "cancellation occurred after an actual synthetic read"
    );
    assert_eq!(
        assets[0].head_bytes(),
        0,
        "staged old-state windows were never published"
    );
}

#[test]
fn initialized_start_hint_rebuilds_for_changed_preparation_and_rejects_source_revision_mismatch() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let old = plan(&ir, assets.clone(), 63);
    let new = plan(&ir, assets.clone(), 127);
    let old_recipe =
        InitialStartHints::new(&ir, &old, &assets, RATE, 64, &policy(4096), &|| false).unwrap();
    let new_recipe =
        InitialStartHints::new(&ir, &new, &assets, RATE, 64, &policy(4096), &|| false).unwrap();
    let old_ranges = old_recipe.ranges(64, &|| false).unwrap();
    let new_ranges = new_recipe.ranges(64, &|| false).unwrap();
    assert_ne!(old_ranges, new_ranges);
    assert!(old_ranges[0].iter().any(|range| range.contains(&5952)));
    assert!(new_ranges[0].iter().any(|range| range.contains(&12_000)));
    let replacement = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    assert_ne!(assets[0].asset_id(), replacement[0].asset_id());
    assert_eq!(
        InitialStartHints::new(&ir, &old, &replacement, RATE, 64, &policy(4096), &|| false)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        InitialStartHints::new(&ir, &old, &assets, 44_100, 64, &policy(4096), &|| false)
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(assets[0].head_bytes() + replacement[0].head_bytes(), 0);
}

#[test]
fn initialized_start_hint_does_not_enable_unproved_native_start_intensity() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let mut route = core::ModRoute::new(0, core::ModTarget::SampleStart, 0.8);
    route.scale = Some(core::ModScale {
        source: 1,
        shape: None,
        lag: 0,
        law: core::ModScaleLaw::KontaktIntensity {
            depth: 0.25,
            flags: 0x10,
            unit: 1.,
        },
    });
    let result = plan(&ir, assets, 63).with_voice_modulation(
        vec![core::ModProgram {
            sources: vec![core::ModSource::Controller(113), core::ModSource::Constant],
            routes: vec![route],
            ..Default::default()
        }],
        vec![Some(0)],
        vec![12_000],
    );
    assert_eq!(result.err().unwrap(), core::Error::InvalidInput);
}

#[test]
fn initialized_start_hint_sample_revision_cannot_reuse_previous_asset_readiness() {
    let ir = instrument(false, 1., 0, None, 12_000);
    let assets = vec![Pcm::streamed(RATE, LENGTH).unwrap()];
    let target = core::SampleTargetId(7);
    let prepared = plan(&ir, assets.clone(), 63)
        .with_sample_targets(
            vec![core::SampleTarget {
                id: target,
                regions: vec![0].into(),
            }],
            core::SampleRevisionLimits {
                slots: 2,
                resident_bytes: 4096,
            },
        )
        .unwrap();
    let (_streamer, cache, _) =
        preload(&ir, &prepared, &assets, &policy(4096), 64, None, &|| false).unwrap();
    assert!(assets[0].resident_covers(5952..5953));
    let (mut actual, mut control) = runtime(prepared, cache).with_sample_updates(2).unwrap();
    control.set_wake(std::thread::current()).unwrap();
    let replacement = Pcm::streamed(RATE, LENGTH).unwrap();
    replacement
        .bind_head_budget(&control.physical_budget())
        .unwrap();
    let revision = || {
        core::PreparedSampleRevision::new(
            target,
            replacement.clone(),
            core::Playback::default(),
            RATE,
        )
        .unwrap()
    };
    let canceled = Arc::new(core::SampleTask::default());
    control.submit(revision(), canceled.clone()).unwrap();
    assert!(canceled.cancel());
    assert!(actual.poll_sample_update().unwrap());
    assert!(control.reply().unwrap().result.is_err());
    assert_eq!(
        actual.sample_target_asset(target).unwrap().asset_id(),
        assets[0].asset_id()
    );
    let committed = Arc::new(core::SampleTask::default());
    control.submit(revision(), committed.clone()).unwrap();
    assert!(actual.poll_sample_update().unwrap());
    assert!(control.reply().unwrap().result.is_ok());
    assert!(committed.success());
    assert_eq!(
        actual.sample_target_asset(target).unwrap().asset_id(),
        replacement.asset_id()
    );
    actual.trigger(input(), 60, 1.).unwrap();
    let mut found = false;
    actual
        .visit_stream_demand(32, |demand| {
            assert_eq!(demand.key.asset, replacement.asset_id());
            assert!(!demand.resident);
            found |= demand.frames.contains(&5952);
            true
        })
        .unwrap();
    assert!(found);
    let mut output = [[1.; 2]; 32];
    actual.render(&mut output).unwrap();
    assert_eq!(output, [[0.; 2]; 32]);
    assert_eq!(actual.stats().cold_starts, 1);
    assert!(
        assets[0].resident_covers(5952..5953),
        "retained old-source storage is never replacement-source readiness"
    );
}

/// One zone per entry, each with a route from `source` to sample start over `range` frames.
/// Mirrors the probe's `cc_start` (CC113) and `key_position` (Key) shapes; the probe
/// creator writes no sample-start routes, so the routes are authored here.
fn start_instrument(source: ir::ModulationSource, range: u64, zones: usize) -> ir::Instrument {
    let zone = |_| {
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.keys = ir::KeyRange { low: 60, high: 60 };
        zone.playback = ir::Playback {
            start_range: range,
            ..Default::default()
        };
        zone.routes.push(ir::RouteRef(0));
        zone
    };
    ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("start-range".into()),
            encoding: ir::Encoding::Unknown,
            residency: ir::AssetResidency::Streamable,
            root_key: None,
            loops: vec![],
        }],
        zones: (0..zones).map(zone).collect(),
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source,
        }],
        routes: vec![ir::Route::new(
            ir::ModulatorRef(0),
            ir::Target::SampleStart,
            ir::Depth::Normalized(1.),
        )],
        ..Default::default()
    }
}

#[test]
fn start_range_cc_start_zones_reach_the_s_mod_range() {
    // Areia pattern: three mic zones, each started by CC113.
    let instrument = start_instrument(ir::ModulationSource::Controller(113), 4_800, 3);
    for zone in &instrument.zones {
        assert_eq!(
            StartRange::of(&instrument, zone),
            StartRange {
                max_frames: 4_800,
                unbounded: false,
                sources: vec![ir::RouteRef(0)],
            }
        );
    }
}

#[test]
fn start_range_key_position_zones_reach_the_s_mod_range() {
    // Vista pattern: two velocity layers started by KeyPosition (ModulationSource::Key).
    let instrument = start_instrument(ir::ModulationSource::Key, 2_400, 2);
    let ranges: Vec<_> = instrument
        .zones
        .iter()
        .map(|zone| StartRange::of(&instrument, zone))
        .collect();
    assert_eq!(
        ranges.iter().map(|r| r.max_frames).collect::<Vec<_>>(),
        [2_400, 2_400]
    );
    assert!(
        ranges
            .iter()
            .all(|r| !r.unbounded && r.sources == [ir::RouteRef(0)])
    );
}

#[test]
fn start_range_without_a_sample_start_route_is_zero() {
    let mut instrument = start_instrument(ir::ModulationSource::Controller(113), 4_800, 1);
    instrument.routes[0].target = ir::Target::Pitch;
    let range = StartRange::of(&instrument, &instrument.zones[0]);
    assert_eq!(range, StartRange::default());
}
