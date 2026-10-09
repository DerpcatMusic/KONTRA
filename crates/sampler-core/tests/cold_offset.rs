use sampler_core::*;
mod support;

fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(17),
    }
}

fn plan(pcm: Pcm, direction: Direction) -> Prepared {
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
            envelope: Envelope::new(0, 0, 0, 1., 128).unwrap(),
            playback: Playback {
                direction,
                ..Default::default()
            },
        }],
        8,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant])
    .unwrap()
    .with_voice_modulation(
        vec![ModProgram {
            sources: vec![ModSource::Constant],
            routes: vec![ModRoute::new(0, ModTarget::SampleStart, 1.)],
            ..Default::default()
        }],
        vec![Some(0)],
        vec![PAGE_FRAMES as u32 * 2],
    )
    .unwrap()
}

fn player(plan: Prepared) -> Runtime {
    let limits = Limits::for_plan(&plan, 1, 1);
    Runtime::new(plan, limits).unwrap()
}

fn late_offset(note_off_before_data: bool) {
    let data: Vec<Frame> = (0..PAGE_FRAMES * 5 + 137)
        .map(|i| [i as f32 / 32768., -(i as f32) / 32768.])
        .collect();
    for direction in [Direction::Forward, Direction::Reverse] {
        let pcm = Pcm::streamed(48000, data.len()).unwrap();
        let start = if direction == Direction::Forward {
            0
        } else {
            data.len() - 4048
        };
        pcm.set_ranges(vec![(
            start,
            data[start..start + 4048].to_vec().into_boxed_slice(),
        )])
        .unwrap();
        let (cache, mut worker) = StreamCache::new(4).unwrap();
        let mut streamed = player(plan(pcm.clone(), direction)).with_stream_cache(cache);
        streamed.set_cold_starts(true);
        let mut resident = player(plan(
            Pcm::new(48000, data.clone().into_boxed_slice()).unwrap(),
            direction,
        ));
        streamed.trigger(input(), 60, 1.).unwrap();
        resident.trigger(input(), 60, 1.).unwrap();
        if note_off_before_data {
            support::without_heap(|| {
                streamed.note_off(input(), None).unwrap();
                resident.note_off(input(), None).unwrap();
            });
            assert_eq!(
                streamed.voice_count(),
                1,
                "a released cold staccato still has an onset"
            );
        }
        let mut silent = [[0.; 2]; 64];
        // More than the old 50 ms hold: a late page must not skip the onset.
        support::without_heap(|| {
            for _ in 0..128 {
                let _ = streamed.service_streaming(128);
                silent.fill([0.; 2]);
                streamed.render(&mut silent).unwrap();
                assert!(silent.iter().all(|f| *f == [0.; 2]));
            }
        });
        while let Some(mut job) = worker.next_job() {
            let range = job.range();
            job.frames_mut().copy_from_slice(&data[range]);
            worker.complete(job, Ok(())).unwrap();
        }
        let mut actual = [[0.; 2]; 64];
        let mut expected = [[0.; 2]; 64];
        support::without_heap(|| {
            streamed.service_streaming(128).unwrap();
            streamed.render(&mut actual).unwrap();
            resident.render(&mut expected).unwrap();
        });
        // The recovery fade lasts 48 frames; after it, the very same source
        // position must match the resident note, rather than a later transient.
        assert_eq!(
            &actual[48..],
            &expected[48..],
            "{direction:?}: the intended offset must be held"
        );
        assert!(actual[48..].iter().any(|f| *f != [0.; 2]));
        if note_off_before_data {
            support::without_heap(|| streamed.render(&mut actual).unwrap());
            assert_eq!(
                streamed.voice_count(),
                0,
                "release clocks 128 active frames, not the storage wait"
            );
        }
    }
}

#[test]
fn an_unpreloaded_start_offset_waits_without_skipping_or_changing_direction() {
    late_offset(false)
}

#[test]
fn staccato_released_before_late_onset_plays_its_correct_offset_then_releases() {
    late_offset(true)
}
