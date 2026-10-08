use sampler_core::*;
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(7),
    }
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
fn runtime(samples: Vec<Pcm>, regions: Vec<Region>) -> Runtime {
    let plan = Prepared::new(48000, samples, regions, 64).unwrap();
    let limits = Limits::for_plan(&plan, 4, 64);
    Runtime::new(plan, limits).unwrap()
}
fn fill(worker: &mut StreamWorker, data: &[Frame]) {
    while let Some(mut job) = worker.next_voice_job() {
        job.fill(|at| Ok(data[at])).unwrap();
        worker.complete_voice(job, Ok(()));
    }
}

#[test]
fn rings_match_resident_resampling_direction_serial_loops_release_and_parallel_render() {
    let data: Box<[Frame]> = (0..20000)
        .map(|i| [(i as f32 * 0.071).sin(), (i as f32 * 0.013).cos()])
        .collect();
    for threads in [1, 2] {
        for step in [0.5_f64, 1., 3.25, 16.] {
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
                    tuning: if step == 16. { 1. } else { 1.25 },
                });
                let playback = Playback {
                    direction,
                    loop_slots: slots,
                    transpose_semitones: 12. * step.log2(),
                    ..Default::default()
                };
                let pcm = Pcm::streamed(48000, data.len()).unwrap();
                let head = if direction == Direction::Forward {
                    0
                } else {
                    data.len() - 4048
                };
                pcm.set_ranges(vec![(
                    head,
                    data[head..head + 4048].to_vec().into_boxed_slice(),
                )])
                .unwrap();
                let (cache, mut worker) = StreamCache::voice_rings(64, 1).unwrap();
                let mut layer = region(0, playback);
                layer.gain = 1. / 64.;
                let regions = vec![layer; if threads == 1 { 2 } else { 64 }];
                let mut streamed = runtime(vec![pcm], regions.clone())
                    .with_stream_cache(cache)
                    .with_threads(Threads::Fixed(threads));
                let mut resident = runtime(vec![Pcm::new(48000, data.clone()).unwrap()], regions)
                    .with_threads(Threads::Fixed(threads));
                let (a, b) = support_trigger(&mut streamed, &mut resident);
                for block in 0..180 {
                    if block == 95 {
                        support::without_heap(|| {
                            streamed.release(a).unwrap();
                            resident.release(b).unwrap();
                        });
                    }
                    support::without_heap(|| {
                        streamed.service_streaming(256).unwrap();
                    });
                    fill(&mut worker, &data);
                    let (mut actual, mut expected) = ([[0.; 2]; 32], [[0.; 2]; 32]);
                    support::without_heap(|| {
                        streamed.service_streaming(256).unwrap();
                        streamed.render(&mut actual).unwrap();
                        resident.render(&mut expected).unwrap();
                    });
                    let worst = actual
                        .iter()
                        .flatten()
                        .zip(expected.iter().flatten())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0f32, f32::max);
                    assert!(
                        worst < 1e-5,
                        "threads={threads} step={step} {direction:?} block={block}: {worst}"
                    );
                    assert_eq!(streamed.stream_underruns(), 0);
                    assert_eq!(streamed.voice_count(), resident.voice_count());
                }
                if threads == 2 {
                    assert!(
                        streamed.parallel_blocks() > 0,
                        "the ring snapshot must reach the render pool"
                    );
                }
            }
        }
    }
}
fn support_trigger(a: &mut Runtime, b: &mut Runtime) -> (NoteId, NoteId) {
    let (mut x, mut y) = (None, None);
    support::without_heap(|| {
        x = Some(a.trigger(input(), 60, 1.).unwrap());
        y = Some(b.trigger(input(), 60, 1.).unwrap());
    });
    (x.unwrap(), y.unwrap())
}

#[test]
fn ring_admission_is_atomic_and_resident_layers_do_not_take_slots() {
    for resident_layer in [false, true] {
        let pcm = Pcm::streamed(48000, 8192).unwrap();
        pcm.set_ranges(vec![(0, vec![[0.5; 2]; 4048].into_boxed_slice())])
            .unwrap();
        let samples = vec![
            pcm.clone(),
            if resident_layer {
                Pcm::new(48000, vec![[1.; 2]; 8192].into_boxed_slice()).unwrap()
            } else {
                pcm
            },
        ];
        let (mut rt, _worker) = {
            let (cache, worker) = StreamCache::voice_rings(1, 1).unwrap();
            (
                runtime(
                    samples,
                    vec![
                        region(0, Playback::default()),
                        region(1, Playback::default()),
                    ],
                )
                .with_stream_cache(cache),
                worker,
            )
        };
        support::without_heap(|| {
            let result = rt.trigger(input(), 60, 1.);
            if resident_layer {
                assert!(result.is_ok());
                assert_eq!(rt.voice_count(), 2);
            } else {
                assert_eq!(result, Err(Error::Capacity));
                assert_eq!((rt.note_count(), rt.voice_count()), (0, 0));
            }
        });
    }
}

#[test]
fn a_terminal_decode_fault_ends_the_held_ring_voice_without_heap_work() {
    let pcm = Pcm::streamed(48000, 8192).unwrap();
    let (cache, mut worker) = StreamCache::voice_rings(2, 1).unwrap();
    let mut rt =
        runtime(vec![pcm], vec![region(0, Playback::default()); 2]).with_stream_cache(cache);
    rt.set_cold_starts(true);
    support::without_heap(|| {
        rt.trigger(input(), 60, 1.).unwrap();
        rt.service_streaming(128).unwrap();
    });
    let job = worker.next_voice_job().unwrap();
    worker.complete_voice(job, Err(DecodeFailure::InvalidSamples));
    support::without_heap(|| {
        assert_eq!(
            rt.service_streaming(128),
            Err(StreamError::DecodeFailed(DecodeFailure::InvalidSamples))
        );
        assert_eq!(rt.voice_count(), 0);
        rt.render(&mut [[0.; 2]; 64]).unwrap();
    });
}
