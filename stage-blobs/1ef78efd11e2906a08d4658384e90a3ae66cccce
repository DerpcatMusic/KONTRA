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
                    assert_eq!(actual, expected, "{rate} {direction:?} {shape:?}");
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
