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

// Old -> new (P1.3): a starved voice keeps its schedule through the wait instead
// of holding its start offset, so the wait is counted as dropout frames and
// the audio after data arrives is the resident note shifted by that wait.
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
        // 128 blocks (8192 frames) without storage: the schedule runs in silence.
        support::without_heap(|| {
            for _ in 0..128 {
                let _ = streamed.service_streaming(128);
                silent.fill([0.; 2]);
                streamed.render(&mut silent).unwrap();
                assert!(silent.iter().all(|f| *f == [0.; 2]));
            }
        });
        let waited = streamed.stats().dropout_frames;
        if note_off_before_data {
            // The release clocks its 128 frames through the wait, so the voice ends
            // before data arrives: 128 silent frames, then nothing.
            assert_eq!(waited, 128, "the release counts its frames as dropouts");
            assert_eq!(
                streamed.voice_count(),
                0,
                "a release before data ends on schedule, not at data arrival"
            );
        } else {
            assert_eq!(waited, 128 * 64, "every waited frame is a dropout");
        }
        while let Some(mut job) = worker.next_job() {
            let range = job.range();
            job.frames_mut().copy_from_slice(&data[range]);
            worker.complete(job, Ok(())).unwrap();
        }
        let mut actual = [[0.; 2]; 64];
        let mut expected = vec![[0.; 2]; waited as usize + 64];
        support::without_heap(|| {
            streamed.service_streaming(128).unwrap();
            streamed.render(&mut actual).unwrap();
            resident.render(&mut expected).unwrap();
        });
        if note_off_before_data {
            assert_eq!(actual, [[0.; 2]; 64], "the released voice is gone when data arrives");
            assert_eq!(streamed.voice_count(), 0);
        } else {
            // After the 48-frame recovery fade, the streamed voice sits at the same
            // source position as the resident note at schedule time `waited`.
            assert_eq!(
                &actual[48..],
                &expected[waited as usize + 48..],
                "{direction:?}: the start plays the correct offset and direction once data arrives"
            );
            assert!(actual[48..].iter().any(|f| *f != [0.; 2]));
        }
    }
}

#[test]
fn an_unpreloaded_start_offset_keeps_its_schedule_and_direction() {
    late_offset(false)
}

#[test]
fn staccato_released_before_late_data_ends_on_schedule_in_silence() {
    late_offset(true)
}
