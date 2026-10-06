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
fn a_missing_live_page_is_observable_and_does_not_orphan_host_pairing() {
    let asset = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
    let data = vec![[0.75; 2]; PAGE_FRAMES * 2];
    let (mut cache, mut worker) = StreamCache::new(1).unwrap();
    load(&mut cache, &mut worker, &asset, &data, 0);
    let mut rt = runtime(vec![asset], vec![]).with_stream_cache(cache);
    let mut output = [[0.; 2]; 8];
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
        rt.render(&mut output).unwrap();
        assert_eq!(&output[..2], &[[0.75; 2]; 2]);
        assert_eq!(&output[2..], &[[0.; 2]; 6]);
        assert_eq!(rt.stream_underruns(), 1);
        assert!(!rt.voice_active(voice));
        assert_eq!(rt.note_count(), 1);
        rt.render(&mut output).unwrap();
        assert_eq!(rt.stream_underruns(), 1);
        assert_eq!(rt.note_off(input(), None), Ok(note));
    });
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
        rt.render(&mut audio).unwrap();
        assert_eq!(rt.voice_count(), 1);
        rt.render(&mut audio[..3]).unwrap();
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(rt.stream_underruns(), 1);
        rt.render(&mut audio).unwrap();
        assert_eq!(audio, [[0.; 2]; 64]);
    });
}
