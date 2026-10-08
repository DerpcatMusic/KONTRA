use sampler_core::*;
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(7),
    }
}
fn runtime(samples: Vec<Pcm>, regions: Vec<Region>) -> Runtime {
    from_plan(Prepared::new(48000, samples, regions, 8).unwrap())
}
fn from_plan(plan: Prepared) -> Runtime {
    Runtime::new(
        plan,
        Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            voices: 8,
            expressions: 4,
            decisions: 0,
            commands: 8,
            behaviors: 0,
            behavior_cells: 0,
            behavior_fuel: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}
fn region(sample: usize, playback: Playback) -> Region {
    Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::new(3, 0, 5, 0.7, 150).unwrap(),
        playback,
    }
}
fn load(
    cache: &mut StreamCache,
    worker: &mut StreamWorker,
    asset: &Pcm,
    data: &[Frame],
    page: usize,
) {
    assert_eq!(cache.request(asset, page, 0), Ok(PageStatus::Pending));
    let mut job = worker.next_job().unwrap();
    let range = job.range();
    job.frames_mut().copy_from_slice(&data[range]);
    worker.complete(job, Ok(())).unwrap();
    assert_eq!(
        cache.poll(),
        Some(PageUpdate::Loaded(PageKey {
            asset: asset.asset_id(),
            index: page
        }))
    );
}

#[test]
fn paged_audio_matches_resident_across_rates_boundaries_loops_release_and_partitions() {
    let data: Box<[Frame]> = (0..PAGE_FRAMES * 2 + 137)
        .map(|i| [(i as f32 * 0.071).sin(), (i as f32 * 0.013).cos()])
        .collect();
    for rate in [12000, 44100, 48000, 96000, 768000] {
        for direction in [Direction::Forward, Direction::Reverse] {
            for shape in [
                None,
                Some(LoopShape::Wrap),
                Some(LoopShape::PingPong),
                Some(LoopShape::Crossfade { frames: 17 }),
            ] {
                let playback = Playback {
                    loop_slots: [None; 8],
                    start: PAGE_FRAMES - 101,
                    end: Some(PAGE_FRAMES + 111),
                    direction,
                    transpose_semitones: 0.,
                    loop_range: shape.map(|shape| Loop {
                        start: PAGE_FRAMES - 30,
                        end: PAGE_FRAMES + 70,
                        shape,
                        mode: LoopMode::UntilRelease,
                        passes: std::num::NonZeroU32::new(5),
                    }),
                };
                let asset = Pcm::streamed(rate, data.len()).unwrap();
                assert_eq!(asset.resident_frames(), None);
                let (mut cache, mut worker) = StreamCache::new(3).unwrap();
                for page in 0..3 {
                    load(&mut cache, &mut worker, &asset, &data, page);
                }
                let mut paged =
                    runtime(vec![asset], vec![region(0, playback)]).with_stream_cache(cache);
                let mut resident = runtime(
                    vec![Pcm::new(rate, data.clone()).unwrap()],
                    vec![region(0, playback)],
                );
                let (mut actual, mut expected) = ([[0.; 2]; 2048], [[0.; 2]; 2048]);
                support::without_heap(|| {
                    let a = paged.trigger(input(), 60, 1.).unwrap();
                    let b = resident.trigger(input(), 60, 1.).unwrap();
                    paged.render(&mut actual[..73]).unwrap();
                    resident.render(&mut expected[..73]).unwrap();
                    paged.release(a).unwrap();
                    resident.release(b).unwrap();
                    for chunk in actual[73..].chunks_mut(7) {
                        paged.render(chunk).unwrap();
                    }
                    for chunk in expected[73..].chunks_mut(257) {
                        resident.render(chunk).unwrap();
                    }
                    // Runs of frames may use fused multiply-adds (wide CPUs)
                    // while the frames between them do not: equal to rounding.
                    let worst = actual
                        .iter()
                        .flatten()
                        .zip(expected.iter().flatten())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0f32, f32::max);
                    assert!(worst < 1e-5, "{rate} {direction:?} {shape:?}: {worst}");
                    assert_eq!(paged.stream_underruns(), 0);
                    assert_eq!(paged.voice_count(), resident.voice_count());
                });
            }
        }
    }
}

#[test]
fn missing_interpolation_guard_rejects_every_layer_before_note_or_voice_publication() {
    let data = vec![[0.5; 2]; PAGE_FRAMES * 2];
    let asset = Pcm::streamed(96000, data.len()).unwrap();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    load(&mut cache, &mut worker, &asset, &data, 0);
    let playback = Playback {
        start: PAGE_FRAMES - 2,
        ..Playback::default()
    };
    let mut rt = runtime(
        vec![
            Pcm::new(48000, vec![[1.; 2]; 16].into()).unwrap(),
            asset.clone(),
        ],
        vec![region(0, Playback::default()), region(1, playback)],
    )
    .with_stream_cache(cache);
    support::without_heap(|| {
        assert_eq!(rt.trigger(input(), 60, 1.), Err(Error::NotReady));
        assert_eq!(
            (rt.note_count(), rt.voice_count(), rt.pending_commands()),
            (0, 0, 0)
        );
        load(
            rt.stream_cache_mut().unwrap(),
            &mut worker,
            &asset,
            &data,
            1,
        );
        rt.trigger(input(), 60, 1.).unwrap();
        assert_eq!(rt.voice_count(), 2);
        rt.render(&mut [[0.; 2]; 16]).unwrap();
        assert_eq!(rt.stream_underruns(), 0);
    });
    assert!(matches!(Pcm::streamed(0, 1), Err(Error::InvalidInput)));
    assert!(matches!(Pcm::streamed(48000, 0), Err(Error::InvalidInput)));
}

#[test]
fn a_missing_live_page_fades_out_keeps_time_and_fades_back_in_when_the_page_arrives() {
    for block in [1, 7, 64] {
        let asset = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
        let data = vec![[0.75; 2]; PAGE_FRAMES * 2];
        let (mut cache, mut worker) = StreamCache::new(1).unwrap();
        load(&mut cache, &mut worker, &asset, &data, 0);
        let mut rt = runtime(vec![asset.clone()], vec![]).with_stream_cache(cache);
        let mut output = [[0.; 2]; 64];
        support::without_heap(|| {
            let note = rt.note_on(input(), 60, 1.).unwrap();
            let family = rt.create_family(note).unwrap();
            let voice = rt
                .start_family(
                    family,
                    0,
                    0,
                    1.,
                    Envelope::default(),
                    Playback {
                        start: PAGE_FRAMES - 2,
                        ..Playback::default()
                    },
                )
                .unwrap();
            rt.finish_family(family).unwrap();
            for chunk in output[..8].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(&output[..2], &[[0.75; 2]; 2]);
            assert_eq!(rt.stream_underruns(), 1);
            assert!(rt.voice_active(voice));
            assert_eq!(rt.note_count(), 1);
            // The starved voice keeps requesting the page it is waiting for.
            let mut wants_page = false;
            assert!(
                rt.visit_voice_demand(voice, 100, |demand| {
                    wants_page |= demand.frames.end > PAGE_FRAMES;
                    true
                })
                .unwrap()
            );
            assert!(wants_page);
            let cache = rt.stream_cache_mut().unwrap();
            cache
                .invalidate(PageKey {
                    asset: asset.asset_id(),
                    index: 0,
                })
                .unwrap();
            load(cache, &mut worker, &asset, &data, 1);
            for chunk in output[8..].chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
            for (i, frame) in output[2..50].iter().enumerate() {
                let expected = 0.75 * (47 - i) as f32 / 48.;
                assert!(frame.iter().all(|x| (x - expected).abs() < 1e-7));
            }
            // The cursor advanced through the fade; the page is now resident, so
            // playback resumes in time with a one-millisecond fade-in.
            for (i, frame) in output[50..].iter().enumerate() {
                let expected = 0.75 * (i + 1) as f32 / 48.;
                assert!(frame.iter().all(|x| (x - expected).abs() < 1e-7));
            }
            assert!(rt.voice_active(voice));
            assert_eq!(rt.stream_underruns(), 1);
            assert_eq!(rt.note_off(input(), None), Ok(note));
        });
    }
}

#[test]
fn a_starved_source_drains_its_declared_dsp_tail_once_across_callback_boundaries() {
    let asset = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
    let data = vec![[1.; 2]; PAGE_FRAMES * 2];
    let (mut cache, mut worker) = StreamCache::new(1).unwrap();
    load(&mut cache, &mut worker, &asset, &data, 0);
    let playback = Playback {
        start: PAGE_FRAMES - 1,
        ..Playback::default()
    };
    let mut r = region(0, playback);
    r.envelope = Envelope::default();
    let plan = Prepared::new(48000, vec![asset], vec![r], 1)
        .unwrap()
        .with_voice_chains(
            vec![
                VoiceChain::new(
                    vec![],
                    vec![Processor::Biquad(
                        Biquad::new(48000, FilterKind::LowPass, 1000., 0.707).unwrap(),
                    )],
                    130,
                )
                .unwrap(),
            ],
            vec![Some(0)],
        )
        .unwrap();
    let mut rt = from_plan(plan).with_stream_cache(cache);
    support::without_heap(|| {
        rt.trigger(input(), 60, 1.).unwrap();
        let mut audio = [[0.; 2]; 64];
        rt.render(&mut audio).unwrap();
        assert_eq!(rt.stream_underruns(), 1);
        assert_eq!(rt.voice_count(), 1);
        assert!(audio[1..].iter().flatten().any(|x| x.abs() > 0.0001));
        // The voice waits silently for the page until its source would have
        // ended (PAGE_FRAMES + 1 frames), then drains the 130-frame DSP tail.
        let mut rendered = 64;
        let end = PAGE_FRAMES + 1 + 130;
        while rendered + 64 < end {
            rt.render(&mut audio).unwrap();
            rendered += 64;
            assert_eq!(rt.voice_count(), 1);
            // Only the low-pass ringing of the fade remains while waiting.
            if rendered > 64 * 4 {
                assert!(audio.iter().flatten().all(|x| x.abs() < 1e-6));
            }
        }
        rt.render(&mut audio[..end - rendered - 1]).unwrap();
        assert_eq!(rt.voice_count(), 1);
        rt.render(&mut audio[..1]).unwrap();
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(rt.stream_underruns(), 1);
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 64]);
    });
}

#[test]
fn live_demand_refills_a_small_cache_across_long_forward_reverse_and_resampled_sources() {
    let data: Box<[Frame]> = (0..PAGE_FRAMES * 10 + 13)
        .map(|i| [(i as f32 * 0.13).sin(), (i as f32 * 0.023).cos()])
        .collect();
    for rate in [44100, 48000, 768000] {
        for direction in [Direction::Forward, Direction::Reverse] {
            let asset = Pcm::streamed(rate, data.len()).unwrap();
            let (mut cache, mut worker) = StreamCache::new(3).unwrap();
            for page in if direction == Direction::Forward {
                [0, 1]
            } else {
                [10, 9]
            } {
                load(&mut cache, &mut worker, &asset, &data, page);
            }
            let playback = Playback {
                direction,
                ..Playback::default()
            };
            let mut mapped = region(0, playback);
            mapped.envelope = Envelope::default();
            let mut rt = runtime(vec![asset.clone()], vec![mapped]).with_stream_cache(cache);
            let mut reference = runtime(vec![Pcm::new(rate, data.clone()).unwrap()], vec![mapped]);
            let (mut actual, mut expected) = ([[0.; 2]; 128], [[0.; 2]; 128]);
            let mut decodes = 0;
            support::without_heap(|| {
                rt.trigger(input(), 60, 1.).unwrap();
                reference.trigger(input(), 60, 1.).unwrap();
                while reference.voice_count() != 0 {
                    let before = rt.now();
                    rt.service_streaming(128).unwrap();
                    while let Some(mut job) = worker.next_job() {
                        assert_eq!(job.key().asset, asset.asset_id());
                        assert!(job.deadline() >= before && job.deadline() < before + 128);
                        let range = job.range();
                        job.frames_mut().copy_from_slice(&data[range]);
                        worker.complete(job, Ok(())).unwrap();
                        decodes += 1;
                    }
                    assert_eq!(rt.service_streaming(128), Ok(true));
                    assert_eq!(rt.now(), before);
                    rt.render(&mut actual).unwrap();
                    reference.render(&mut expected).unwrap();
                    assert_eq!(actual, expected, "{rate} {direction:?} at {before}");
                }
                assert_eq!(rt.voice_count(), 0);
                assert_eq!(rt.stream_underruns(), 0);
            });
            assert_eq!(decodes, 9, "each additional asset page decoded once");
        }
    }
}

#[test]
fn stream_service_protects_all_voices_before_eviction_and_restores_cache_after_capacity_errors() {
    let data = vec![[0.5; 2]; PAGE_FRAMES * 2];
    let a = Pcm::streamed(48000, data.len()).unwrap();
    let b = Pcm::streamed(48000, data.len()).unwrap();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    load(&mut cache, &mut worker, &a, &data, 0);
    load(&mut cache, &mut worker, &b, &data, 0);
    let mut rt = runtime(
        vec![a.clone(), b.clone()],
        vec![
            region(0, Playback::default()),
            region(1, Playback::default()),
        ],
    )
    .with_stream_cache(cache);
    support::without_heap(|| {
        rt.trigger(input(), 60, 1.).unwrap();
        assert_eq!(
            rt.service_streaming(PAGE_FRAMES as u32 + 1),
            Err(StreamError::Capacity)
        );
        for asset in [&a, &b] {
            assert_eq!(
                rt.stream_cache_mut().unwrap().status(PageKey {
                    asset: asset.asset_id(),
                    index: 0
                }),
                PageStatus::Ready
            );
        }
        assert_eq!(rt.service_streaming(64), Ok(true));
        assert_eq!(rt.now(), 0);
        assert_eq!(rt.voice_count(), 2);
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        assert_eq!(rt.stream_underruns(), 0);
    });
    let mut unconfigured = runtime(vec![], vec![]);
    assert_eq!(
        unconfigured.service_streaming(1),
        Err(StreamError::NotConfigured)
    );
}

#[test]
fn starvation_duration_uses_output_rate_and_missing_onsets_do_not_emit_a_fade() {
    for rate in [44100_u32, 96000, 192000] {
        let asset = Pcm::streamed(rate, PAGE_FRAMES * 2).unwrap();
        let data = vec![[1., -0.4]; PAGE_FRAMES * 2];
        let (mut cache, mut worker) = StreamCache::new(1).unwrap();
        load(&mut cache, &mut worker, &asset, &data, 0);
        let mut r = region(
            0,
            Playback {
                start: PAGE_FRAMES - 2,
                ..Playback::default()
            },
        );
        r.envelope = Envelope::default();
        let data_asset = asset.asset_id();
        let mut rt = from_plan(Prepared::new(rate, vec![asset], vec![r], 1).unwrap())
            .with_stream_cache(cache);
        let fade = rate.div_ceil(1000) as usize;
        let mut output = vec![[0.; 2]; fade + 10];
        support::without_heap(|| {
            rt.trigger(input(), 60, 1.).unwrap();
            for chunk in output.chunks_mut(1) {
                rt.render(chunk).unwrap();
            }
            assert_eq!(&output[..2], &[[1., -0.4]; 2]);
            assert!(output[2][0] < 1. && output[2][0] > 0.97);
            assert!(
                output[2..fade + 1]
                    .windows(2)
                    .all(|w| w[1][0] < w[0][0] && w[1][1] > w[0][1])
            );
            assert_eq!(&output[fade + 1..], &[[0.; 2]; 9]);
            assert_eq!(rt.stream_underruns(), 1);
            // Still waiting for page 1, silently.
            assert_eq!(rt.voice_count(), 1);
            rt.stream_cache_mut()
                .unwrap()
                .invalidate(PageKey {
                    asset: data_asset,
                    index: 0,
                })
                .unwrap();
            assert_eq!(rt.trigger(input(), 60, 1.), Err(Error::NotReady));
            rt.render(&mut output).unwrap();
            assert!(output.iter().all(|f| *f == [0.; 2]));
        });
    }
}

#[test]
fn a_missing_filter_guard_fades_from_the_last_complete_resample_without_partial_kernel_audio() {
    let data: Box<[Frame]> = (0..PAGE_FRAMES * 2)
        .map(|i| [(i as f32 * 0.05).sin(), (i as f32 * 0.021).cos()])
        .collect();
    let asset = Pcm::streamed(96000, data.len()).unwrap();
    let (mut cache, mut worker) = StreamCache::new(1).unwrap();
    load(&mut cache, &mut worker, &asset, &data, 0);
    let mut mapped = region(
        0,
        Playback {
            start: PAGE_FRAMES - 300,
            ..Playback::default()
        },
    );
    mapped.envelope = Envelope::default();
    // The long (High) kernel's guard is what reaches the missing page here.
    let mut rt = runtime(vec![asset], vec![mapped])
        .with_resample_quality(ResampleQuality::High)
        .with_stream_cache(cache);
    let mut reference = runtime(vec![Pcm::new(96000, data).unwrap()], vec![mapped])
        .with_resample_quality(ResampleQuality::High);
    let (mut actual, mut expected) = ([[0.; 2]; 256], [[0.; 2]; 256]);
    support::without_heap(|| {
        rt.trigger(input(), 60, 1.).unwrap();
        reference.trigger(input(), 60, 1.).unwrap();
        for chunk in actual.chunks_mut(17) {
            rt.render(chunk).unwrap();
        }
        reference.render(&mut expected).unwrap();
        // At 2x, radius 96 first reaches the missing page after 102 output frames.
        assert_eq!(&actual[..102], &expected[..102]);
        for (index, frame) in actual[102..150].iter().enumerate() {
            for channel in 0..2 {
                let faded = expected[101][channel] * (47 - index) as f32 / 48.;
                assert!((frame[channel] - faded).abs() < 1e-7);
            }
        }
        assert!(actual[150..].iter().all(|f| *f == [0.; 2]));
        assert_eq!(rt.stream_underruns(), 1);
        // Waiting for the missing page, not ended.
        assert_eq!(rt.voice_count(), 1);
    });
}

#[test]
fn a_full_pool_reclaims_a_voice_waiting_on_its_stream_before_dropping_starts() {
    let asset = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
    let resident = Pcm::new(48000, vec![[0.5; 2]; 1000].into()).unwrap();
    let data = vec![[0.75; 2]; PAGE_FRAMES * 2];
    let (mut cache, mut worker) = StreamCache::new(1).unwrap();
    load(&mut cache, &mut worker, &asset, &data, 0);
    let plan = Prepared::new(48000, vec![asset, resident], vec![], 8).unwrap();
    let limits = Limits {
        voices: 2,
        ..Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            voices: 8,
            expressions: 4,
            decisions: 0,
            commands: 8,
            behaviors: 0,
            behavior_cells: 0,
            behavior_fuel: 0,
            note_cells: 0,
        }
    };
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    let mut output = [[0.; 2]; 64];
    support::without_heap(|| {
        let note = rt.note_on(input(), 60, 1.).unwrap();
        let family = rt.create_family(note).unwrap();
        let start = |rt: &mut Runtime, sample, start| {
            let playback = Playback {
                start,
                ..Playback::default()
            };
            let at = rt.now();
            rt.start_family(family, sample, at, 1., Envelope::default(), playback)
        };
        let starved = start(&mut rt, 0, PAGE_FRAMES - 2).unwrap();
        let audible = start(&mut rt, 1, 0).unwrap();
        // Two frames, then the one-millisecond fade: the voice is now waiting.
        rt.render(&mut output).unwrap();
        assert_eq!(rt.stats().stream_underruns, 1);
        let reclaimed = start(&mut rt, 1, 0).unwrap();
        assert!(!rt.voice_active(starved));
        assert!(rt.voice_active(audible) && rt.voice_active(reclaimed));
        assert_eq!(start(&mut rt, 1, 0), Err(Error::Capacity));
        let stats = rt.stats();
        assert_eq!((stats.voice_drops, stats.voices), (1, 2));
        assert_eq!(stats.render_frames_last, 64);
        assert!(stats.render_nanos_peak >= stats.render_nanos_last);
        assert_eq!(stats.stream_cache_bytes, PAGE_FRAMES * 8);
        rt.render(&mut output).unwrap();
        assert!(output.iter().all(|f| f == &[1.; 2]));
    });
    assert_eq!(rt.resident_bytes(), 1000 * 8);
}

#[test]
fn a_resident_head_starts_cold_voices_and_streams_the_rest() {
    let data: Vec<Frame> = (0..PAGE_FRAMES * 2 + 100)
        .map(|i| [i as f32 / 1e4, -(i as f32) / 1e4])
        .collect();
    let asset = Pcm::headed(48000, data.len(), &data[..PAGE_FRAMES]).unwrap();
    assert_eq!(asset.resident_bytes(), PAGE_FRAMES * 8);
    let (cache, mut worker) = StreamCache::new(2).unwrap();
    let mut rt =
        runtime(vec![asset.clone()], vec![region(0, Playback::default())]).with_stream_cache(cache);
    let mut output = vec![[0.; 2]; PAGE_FRAMES + 200];
    support::without_heap(|| {
        rt.trigger(input(), 60, 1.).unwrap();
        assert!(!rt.service_streaming(PAGE_FRAMES as u32 + 200).unwrap());
    });
    // The head never travels through the cache: only page 1 is requested.
    let mut job = worker.next_job().unwrap();
    assert_eq!(job.range(), PAGE_FRAMES..PAGE_FRAMES * 2);
    let range = job.range();
    job.frames_mut().copy_from_slice(&data[range]);
    worker.complete(job, Ok(())).unwrap();
    assert!(worker.next_job().is_none());
    support::without_heap(|| {
        assert!(rt.service_streaming(PAGE_FRAMES as u32 + 200).unwrap());
        rt.render(&mut output).unwrap();
    });
    assert_eq!(rt.stream_underruns(), 0);
    let reference = {
        let resident = Pcm::new(48000, data.clone().into()).unwrap();
        let mut rt = runtime(vec![resident], vec![region(0, Playback::default())]);
        let mut output = vec![[0.; 2]; PAGE_FRAMES + 200];
        rt.trigger(input(), 60, 1.).unwrap();
        rt.render(&mut output).unwrap();
        output
    };
    assert_eq!(output, reference);
    // A purged head refuses starts and marks the asset for reloading.
    rt.note_off(input(), None).unwrap();
    assert_eq!(asset.set_ranges(vec![]).unwrap().len(), 1);
    assert!(!asset.take_cold());
    assert_eq!(rt.trigger(input(), 60, 1.), Err(Error::NotReady));
    assert!(asset.take_cold() && !asset.take_cold());
    assert!(asset.last_played() > 0);
}

#[test]
fn a_cold_start_waits_silently_for_its_page_then_fades_in() {
    let data = vec![[0.5; 2]; PAGE_FRAMES * 2];
    let asset = Pcm::streamed(48000, data.len()).unwrap();
    let (cache, mut worker) = StreamCache::new(4).unwrap();
    let mut rt =
        runtime(vec![asset.clone()], vec![region(0, Playback::default())]).with_stream_cache(cache);
    assert_eq!(rt.trigger(input(), 60, 1.), Err(Error::NotReady));
    rt.set_cold_starts(true);
    let mut output = vec![[0.; 2]; 256];
    support::without_heap(|| {
        rt.trigger(input(), 60, 1.).unwrap();
        assert!(!rt.service_streaming(PAGE_FRAMES as u32).unwrap());
        rt.render(&mut output).unwrap();
    });
    assert!(output.iter().all(|f| f == &[0.; 2]));
    assert!(asset.take_cold());
    let mut job = worker.next_job().unwrap();
    let range = job.range();
    job.frames_mut().copy_from_slice(&data[range]);
    worker.complete(job, Ok(())).unwrap();
    support::without_heap(|| {
        rt.service_streaming(PAGE_FRAMES as u32).unwrap();
        rt.render(&mut output).unwrap();
    });
    // A short fade-in, then the source at full level.
    assert!(
        output[0][0] >= 0.
            && output[10][0] > 0.
            && output[0][0] < output[255][0]
            && output[60] == output[255]
    );
    assert!(output.windows(2).all(|w| w[0][0] <= w[1][0]));
    let stats = rt.stats();
    assert_eq!((stats.cold_starts, stats.stream_underruns), (1, 0));
}

#[test]
fn a_lazy_start_preserves_its_attack_until_the_first_page_arrives() {
    let mut data = vec![[0.; 2]; PAGE_FRAMES * 2];
    data[..128].fill([0.5; 2]); // A transient that a silent advancing cursor loses.
    let asset = Pcm::streamed(48000, data.len()).unwrap();
    let (cache, mut worker) = StreamCache::new(4).unwrap();
    let mut r = region(0, Playback::default());
    r.envelope = Envelope::default();
    let mut rt = runtime(vec![asset], vec![r]).with_stream_cache(cache);
    rt.set_cold_starts(true);
    let mut output = [[0.; 2]; 256];
    rt.trigger(input(), 60, 1.).unwrap();
    rt.service_streaming(PAGE_FRAMES as u32).unwrap();
    support::without_heap(|| rt.render(&mut output).unwrap());
    assert!(output.iter().all(|f| f == &[0.; 2]));
    let mut job = worker.next_job().unwrap();
    let range = job.range();
    job.frames_mut().copy_from_slice(&data[range]);
    worker.complete(job, Ok(())).unwrap();
    rt.service_streaming(PAGE_FRAMES as u32).unwrap();
    support::without_heap(|| rt.render(&mut output).unwrap());
    assert!(
        output.iter().any(|f| f[0] > 0.4),
        "the held attack must still play"
    );
    assert_eq!(rt.stats().stream_underruns, 0);
}

#[test]
fn a_resident_range_at_a_zone_start_admits_that_zone_only() {
    let data = vec![[0.5; 2]; PAGE_FRAMES * 3];
    let asset = Pcm::streamed(48000, data.len()).unwrap();
    let range = data[PAGE_FRAMES..PAGE_FRAMES + 300].into();
    asset.set_ranges(vec![(PAGE_FRAMES, range)]).unwrap();
    assert!(
        asset
            .set_ranges(vec![(PAGE_FRAMES * 3, [[0.; 2]].into())])
            .is_err()
    );
    assert!(
        asset
            .set_ranges(vec![(0, [[0.; 2]; 2].into()), (1, [[0.; 2]].into())])
            .is_err()
    );
    let at = |start| {
        let playback = Playback {
            start,
            ..Playback::default()
        };
        let cache = StreamCache::new(2).unwrap().0;
        runtime(vec![asset.clone()], vec![region(0, playback)]).with_stream_cache(cache)
    };
    let mut inside = at(PAGE_FRAMES + 100);
    let mut output = [[0.; 2]; 64];
    inside.trigger(input(), 60, 1.).unwrap();
    inside.render(&mut output).unwrap();
    assert!(output[10..].iter().all(|f| f[0] > 0.));
    assert_eq!(inside.stream_underruns(), 0);
    assert_eq!(at(100).trigger(input(), 60, 1.), Err(Error::NotReady));
}

#[test]
fn a_full_pool_steals_instead_of_refusing_or_panicking() {
    let asset = Pcm::new(48000, vec![[0.5; 2]; PAGE_FRAMES].into()).unwrap();
    let plan = Prepared::new(48000, vec![asset], vec![region(0, Playback::default())], 8).unwrap();
    let limits = Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 4,
        voices: 2,
        expressions: 4,
        decisions: 0,
        commands: 8,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
        note_cells: 0,
    };
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut output = [[0.; 2]; 64];
    // Every voice audible: the third start steals one rather than failing.
    for id in 1..=3 {
        let held = Input {
            external_id: Some(id),
            ..input()
        };
        let _ = rt.trigger(held, 60, 1.);
    }
    rt.render(&mut output).unwrap();
    let stats = rt.stats();
    assert_eq!(stats.voices, 2);
    assert_eq!(stats.voice_drops + stats.refused_starts, 0);
}

#[test]
fn a_start_offset_into_a_missing_page_is_refused_counted_and_marks_the_asset_cold() {
    // Selection preflights the region's first window (page 0, resident); the
    // sample-start route then moves the voice into page 2, which is not.
    let data = vec![[0.5; 2]; PAGE_FRAMES * 3];
    let asset = Pcm::streamed(48000, data.len()).unwrap();
    let (mut cache, mut worker) = StreamCache::new(4).unwrap();
    load(&mut cache, &mut worker, &asset, &data, 0);
    let program = ModProgram {
        breakpoints: vec![],
        sources: vec![ModSource::Velocity],
        routes: vec![ModRoute::new(0, ModTarget::SampleStart, 1.)],
        shapes: vec![],
    };
    let plan = Prepared::new(
        48000,
        vec![asset.clone()],
        vec![region(0, Playback::default())],
        8,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant])
    .unwrap()
    .with_voice_modulation(vec![program], vec![Some(0)], vec![PAGE_FRAMES as u32 * 2])
    .unwrap();
    let mut rt = from_plan(plan).with_stream_cache(cache);
    rt.set_cold_starts(false);
    assert!(!asset.take_cold());
    let mut output = [[0.; 2]; 64];
    rt.trigger(input(), 60, 1.).unwrap();
    rt.render(&mut output).unwrap();
    let stats = rt.stats();
    assert_eq!((stats.refused_starts, stats.voices), (1, 0));
    assert!(output.iter().all(|f| f == &[0.; 2]));
    assert!(
        asset.take_cold(),
        "the refusal asks for the page to be loaded"
    );
}

#[test]
fn tuned_serial_loops_match_resident_audio_and_demand_at_every_partition() {
    let data: Box<[Frame]> = (0..PAGE_FRAMES * 2)
        .map(|i| [(i as f32 * 0.071).sin(), (i as f32 * 0.013).cos()])
        .collect();
    for step in [0.5_f64, 1., 3.25] {
        for direction in [Direction::Forward, Direction::Reverse] {
            let mut slots = [None; 8];
            slots[7] = Some(LoopSlot {
                range: Loop {
                    start: PAGE_FRAMES - 50,
                    end: PAGE_FRAMES - 10,
                    mode: LoopMode::UntilRelease,
                    shape: LoopShape::PingPong,
                    passes: std::num::NonZeroU32::new(3),
                },
                tuning: 0.75,
            });
            slots[1] = Some(LoopSlot {
                range: Loop {
                    start: PAGE_FRAMES + 30,
                    end: PAGE_FRAMES + 70,
                    mode: LoopMode::UntilRelease,
                    shape: LoopShape::Crossfade { frames: 7 },
                    passes: std::num::NonZeroU32::new(2),
                },
                tuning: 1.25,
            });
            let playback = Playback {
                start: PAGE_FRAMES - 100,
                end: Some(PAGE_FRAMES + 100),
                direction,
                loop_slots: slots,
                transpose_semitones: 12. * step.log2(),
                ..Default::default()
            };
            let asset = Pcm::streamed(48000, data.len()).unwrap();
            let (mut cache, mut worker) = StreamCache::new(2).unwrap();
            for page in 0..2 {
                load(&mut cache, &mut worker, &asset, &data, page);
            }
            let mut paged =
                runtime(vec![asset], vec![region(0, playback)]).with_stream_cache(cache);
            let mut resident = runtime(
                vec![Pcm::new(48000, data.clone()).unwrap()],
                vec![region(0, playback)],
            );
            let (mut actual, mut expected) = ([[0.; 2]; 1024], [[0.; 2]; 1024]);
            support::without_heap(|| {
                let a = paged.trigger(input(), 60, 1.).unwrap();
                let b = resident.trigger(input(), 60, 1.).unwrap();
                paged.render(&mut actual[..173]).unwrap();
                resident.render(&mut expected[..173]).unwrap();
                paged.release(a).unwrap();
                resident.release(b).unwrap();
                for chunk in actual[173..].chunks_mut(7) {
                    paged.render(chunk).unwrap();
                }
                for chunk in expected[173..].chunks_mut(257) {
                    resident.render(chunk).unwrap();
                }
                assert_eq!(actual, expected, "step={step} {direction:?}");
                assert_eq!(paged.stream_underruns(), 0);
            });
        }
    }
}

#[test]
fn cold_start_holds_the_chain_envelope_until_its_first_page_arrives_without_heap() {
    for chained in [false, true] {
        for voices in [1, 2] {
            let asset = Pcm::streamed(48000, PAGE_FRAMES).unwrap();
            let data = vec![[0.25; 2]; PAGE_FRAMES];
            let build = || {
                let mut mapped = region(0, Playback::default());
                mapped.envelope = Envelope::new(512, 0, 128, 0.5, 64).unwrap();
                let plan =
                    Prepared::new(48000, vec![asset.clone()], vec![mapped; voices], 8).unwrap();
                let plan = if chained {
                    plan.with_voice_chains(
                        vec![VoiceChain::new(vec![Processor::Gain(1.)], vec![], 0).unwrap()],
                        vec![Some(0); voices],
                    )
                    .unwrap()
                } else {
                    plan
                };
                let (cache, worker) = StreamCache::new(8).unwrap();
                let mut rt = from_plan(plan).with_stream_cache(cache);
                rt.set_cold_starts(true);
                support::without_heap(|| {
                    rt.trigger(input(), 60, 1.).unwrap();
                });
                (rt, worker)
            };
            let (mut delayed, mut delayed_worker) = build();
            let (mut immediate, mut immediate_worker) = build();
            // Both are admitted cold and take the same one-millisecond fade-in.
            load(
                immediate.stream_cache_mut().unwrap(),
                &mut immediate_worker,
                &asset,
                &data,
                0,
            );
            let mut silence = [[0.; 2]; 128];
            support::without_heap(|| delayed.render(&mut silence).unwrap());
            assert_eq!(silence, [[0.; 2]; 128]);
            load(
                delayed.stream_cache_mut().unwrap(),
                &mut delayed_worker,
                &asset,
                &data,
                0,
            );
            let (mut actual, mut expected) = ([[0.; 2]; 256], [[0.; 2]; 256]);
            support::without_heap(|| {
                delayed.render(&mut actual).unwrap();
                immediate.render(&mut expected).unwrap();
            });
            assert!(expected.iter().flatten().any(|x| *x != 0.));
            assert_eq!(actual, expected, "chained {chained}, voices {voices}");
            assert_eq!(delayed.stream_underruns(), 0);
            assert_eq!(immediate.stream_underruns(), 0);
        }
    }
}
