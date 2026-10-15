mod support;
use sampler_core::{Error, HeadBudget, Pcm};

#[test]
fn exact_reservation_precedes_payload_allocation_and_covers_all_lossless_widths() {
    let frames = [[0.5; 2]; 8];
    let zero = HeadBudget::new(0);
    let pcm = Pcm::streamed(48000, 64).unwrap();
    pcm.bind_head_budget(&zero).unwrap();
    let mut builder = pcm.head_preparer().builder();
    support::without_heap(|| assert_eq!(builder.push(0, &frames), Err(Error::Capacity)));
    support::without_heap(|| assert!(!pcm.resident_covers(0..8)));
    assert_eq!(zero.status().held_bytes, 0);
    assert_eq!(zero.status().peak_bytes, 0);
    for (frames, bytes) in [
        (vec![[0.5; 2]; 8], 16),
        (vec![[0.5, -0.5]; 8], 32),
        (vec![[1.0 / 8388608.0, 0.5]; 8], 48),
        (vec![[0.1234567, -0.1234567]; 8], 64),
        (vec![[-0.0, 0.0]; 8], 64),
    ] {
        support::without_heap(|| assert_eq!(sampler_core::Packed::prepare(&frames).bytes(), bytes));
        let mut packed = None;
        assert_eq!(
            support::allocated_bytes(|| packed = Some(sampler_core::Packed::new(&frames))),
            bytes,
            "packing allocates only its final lossless payload"
        );
        drop(packed);
        let budget = HeadBudget::new(bytes);
        let pcm = Pcm::streamed(48000, 64).unwrap();
        pcm.bind_head_budget(&budget).unwrap();
        let prepared = pcm
            .prepare_ranges(vec![(0, frames.into_boxed_slice())])
            .unwrap();
        assert_eq!(prepared.bytes(), bytes);
        assert_eq!(budget.status().held_bytes, bytes);
        assert_eq!(pcm.head_bytes(), 0, "staging is inert but charged");
        pcm.publish_ranges(prepared).unwrap();
        assert_eq!(pcm.head_bytes(), bytes);
        assert_eq!(budget.status().held_bytes, bytes);
        let mut over = pcm.head_preparer().builder();
        support::without_heap(|| assert_eq!(over.push(0, &[[0.5; 2]; 1]), Err(Error::Capacity)));
        assert_eq!(budget.status().peak_bytes, bytes);
        drop(pcm);
        assert_eq!(budget.status().held_bytes, 0);
    }
}

#[test]
fn retired_storage_stays_charged_after_audio_guard_drop_until_control_reclaims_it() {
    let budget = HeadBudget::new(32);
    let pcm = Pcm::streamed(48000, 64).unwrap();
    pcm.bind_head_budget(&budget).unwrap();
    pcm.set_ranges(vec![(0, vec![[0.25; 2]; 8].into_boxed_slice())])
        .unwrap();
    let retired = pcm.set_ranges(Vec::new()).unwrap();
    assert_eq!(retired[0].1.bytes(), 16);
    assert_eq!(budget.status().held_bytes, 16);
    pcm.set_ranges(vec![(16, vec![[0.5; 2]; 8].into_boxed_slice())])
        .unwrap();
    assert_eq!(budget.status().held_bytes, 32);
    assert_eq!(pcm.head_bytes(), 16);
    let mut denied = pcm.head_preparer().builder();
    support::without_heap(|| assert_eq!(denied.push(32, &[[0.5; 2]; 8]), Err(Error::Capacity)));
    support::without_heap(|| drop(retired));
    assert_eq!(
        budget.status().held_bytes,
        32,
        "audio never frees retired storage or its lease"
    );
    assert_eq!(pcm.head_bytes(), 16);
    assert_eq!(
        budget.status().held_bytes,
        16,
        "only control collection releases quota"
    );
    let staged = pcm
        .prepare_ranges(vec![(32, vec![[0.5; 2]; 8].into_boxed_slice())])
        .unwrap();
    assert_eq!(budget.status().held_bytes, 32);
    drop(staged);
    assert_eq!(budget.status().held_bytes, 16);
    pcm.set_ranges(Vec::new()).unwrap();
    pcm.head_bytes();
    assert_eq!(budget.status().held_bytes, 0);
    assert_eq!(budget.status().peak_bytes, 32);
}

#[test]
fn parallel_staging_cannot_exceed_the_shared_packed_cap_and_failed_transaction_releases_it() {
    let budget = HeadBudget::new(4096 * 2 * 3);
    let pcms: Vec<_> = (0..8)
        .map(|_| {
            let pcm = Pcm::streamed(48000, 8192).unwrap();
            pcm.bind_head_budget(&budget).unwrap();
            pcm
        })
        .collect();
    let barrier = std::sync::Barrier::new(pcms.len());
    std::thread::scope(|scope| {
        let jobs: Vec<_> = pcms
            .iter()
            .map(|pcm| {
                scope.spawn(|| {
                    let frames = [[0.5; 2]; 4096];
                    let mut builder = pcm.head_preparer().builder();
                    let result = builder.push(0, &frames);
                    barrier.wait();
                    assert!(budget.status().held_bytes <= budget.status().limit_bytes);
                    (result, builder.finish())
                })
            })
            .collect();
        let prepared: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
        assert_eq!(prepared.iter().filter(|(r, _)| r.is_ok()).count(), 3);
        assert_eq!(budget.status().held_bytes, 4096 * 2 * 3);
        assert_eq!(budget.status().peak_bytes, 4096 * 2 * 3);
    });
    assert_eq!(budget.status().held_bytes, 0);
    assert!(pcms.iter().all(|pcm| pcm.head_bytes() == 0));
}

#[test]
fn binding_rejects_stale_unbudgeted_and_wrong_budget_transactions() {
    let pcm = Pcm::streamed(48000, 64).unwrap();
    let escaped = pcm.head_preparer();
    let stale = escaped
        .prepare(vec![(0, vec![[0.5; 2]; 8].into_boxed_slice())])
        .unwrap();
    let budget = HeadBudget::new(16);
    pcm.bind_head_budget(&budget).unwrap();
    assert_eq!(
        pcm.publish_ranges(stale).err(),
        Some(Error::RevisionConflict)
    );
    assert_eq!(pcm.head_bytes(), 0);
    let after = escaped
        .prepare(vec![(0, vec![[0.5; 2]; 8].into_boxed_slice())])
        .unwrap();
    assert_eq!(
        pcm.publish_ranges(after).err(),
        Some(Error::RevisionConflict)
    );
    assert_eq!(budget.status().held_bytes, 0);
    assert_eq!(
        pcm.bind_head_budget(&HeadBudget::new(16)),
        Err(Error::RevisionConflict)
    );
    pcm.bind_head_budget(&budget).unwrap();
    let occupied = Pcm::headed(48000, 64, &[[0.5; 2]; 8]).unwrap();
    occupied.bind_head_budget(&budget).unwrap();
    assert_eq!(budget.status().held_bytes, 16);
    assert!(occupied.resident_covers(0..8));
    let too_small = HeadBudget::new(15);
    let other = Pcm::headed(48000, 64, &[[0.5; 2]; 8]).unwrap();
    assert_eq!(other.bind_head_budget(&too_small), Err(Error::Capacity));
    assert!(
        other.resident_covers(0..8),
        "failed admission cannot discard existing storage"
    );
    assert_eq!(too_small.status().held_bytes, 0);
}

#[test]
fn page_bounded_packed_heads_match_resident_pcm_across_reverse_and_loop_seams_without_heap() {
    use sampler_core::{
        Direction, Envelope, Input, Limits, Loop, LoopMode, LoopShape, PAGE_FRAMES, Playback,
        Prepared, Protocol, Region, Runtime,
    };
    let data: Vec<_> = (0..PAGE_FRAMES * 3)
        .map(|i| {
            let phase = i as f32;
            if i < PAGE_FRAMES {
                [((i % 8192) as f32 - 4096.) / 32768.; 2]
            } else if i < PAGE_FRAMES * 2 {
                [(i % 4096) as f32 / 8388608., 0.5]
            } else {
                [(phase / 71.).sin(), (phase / 97.).cos()]
            }
        })
        .collect();
    for direction in [Direction::Forward, Direction::Reverse] {
        for shape in [LoopShape::Wrap, LoopShape::PingPong] {
            let playback = Playback {
                start: PAGE_FRAMES / 2,
                end: Some(PAGE_FRAMES * 3 - 512),
                direction,
                loop_range: Some(Loop {
                    start: PAGE_FRAMES - 23,
                    end: PAGE_FRAMES * 2 + 31,
                    shape,
                    mode: LoopMode::Continuous,
                    passes: None,
                }),
                ..Default::default()
            };
            let render = |pcm: Pcm, partition: usize| {
                let plan = Prepared::new(
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
                        envelope: Envelope::new(0, 0, 0, 1., 8).unwrap(),
                        playback,
                    }],
                    1,
                )
                .unwrap();
                let limits = Limits::for_plan(&plan, 4, 1);
                let (cache,_worker)=sampler_core::StreamCache::new(4).unwrap();
                let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
                let mut audio = vec![[0.; 2]; PAGE_FRAMES * 4 + 127];
                support::without_heap(|| {
                    rt.trigger(
                        Input {
                            protocol: Protocol::Native,
                            port: 0,
                            group: 0,
                            channel: 0,
                            key: 60,
                            external_id: Some(1),
                        },
                        60,
                        1.,
                    )
                    .unwrap();
                    assert_eq!(rt.voice_count(),1);
                    for block in audio.chunks_mut(partition) {
                        rt.render(block).unwrap();
                    }
                    assert_eq!(rt.stream_underruns(), 0);
                });
                assert!(audio.iter().flatten().any(|v|v.abs()>0.01),"the PCM oracle must actually sound");
                audio
            };
            let expected = render(
                Pcm::new(48000, data.clone().into_boxed_slice()).unwrap(),
                128,
            );
            for partition in [7, 128] {
                let budget = HeadBudget::new(data.len() * 8);
                let pcm = Pcm::streamed(48000, data.len()).unwrap();
                pcm.bind_head_budget(&budget).unwrap();
                let mut builder = pcm.head_preparer().builder();
                for (page, frames) in data.chunks(PAGE_FRAMES).enumerate() {
                    builder.push(page * PAGE_FRAMES, frames).unwrap();
                }
                pcm.publish_ranges(builder.finish()).unwrap();
                assert!(pcm.resident_covers(0..data.len()));
                assert_eq!(
                    render(pcm, partition),
                    expected,
                    "{direction:?} {shape:?} partition={partition}"
                );
                assert_eq!(budget.status().held_bytes, 0);
                assert!(budget.status().peak_bytes <= budget.status().limit_bytes);
            }
        }
    }
}
