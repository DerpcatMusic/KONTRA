use sampler_core::{
    Envelope, Error, Input, Limits, Loop, LoopMode, LoopShape, Pcm, Playback, Prepared,
    PreparedSampleRevision, Protocol, Region, Runtime, SampleRevisionLimits, SampleTarget,
    SampleTargetId, SampleTask,
};
use std::sync::Arc;
mod support;
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn playback(start: usize, end: usize) -> Playback {
    Playback {
        start,
        end: Some(end),
        loop_range: Some(Loop {
            start,
            end,
            passes: None,
            mode: LoopMode::Continuous,
            shape: LoopShape::Wrap,
        }),
        ..Default::default()
    }
}
fn prepared(slots: usize, bytes: usize) -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, (0..12).map(|i| [i as f32; 2]).collect()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 1).unwrap(),
            playback: playback(1, 4),
        }],
        4,
    )
    .unwrap()
    .with_sample_targets(
        vec![SampleTarget {
            id: SampleTargetId(7),
            regions: vec![0].into(),
        }],
        SampleRevisionLimits {
            slots,
            resident_bytes: bytes,
        },
    )
    .unwrap()
}
fn revision(value: f32, rate: u32, frames: usize) -> PreparedSampleRevision {
    PreparedSampleRevision::new(
        SampleTargetId(7),
        Pcm::new(rate, (0..frames).map(|i| [value + i as f32; 2]).collect()).unwrap(),
        playback(2, 6),
        48000,
    )
    .unwrap()
}
fn runtime(slots: usize, bytes: usize, queued: usize) -> (Runtime, sampler_core::SampleControl) {
    let (rt, mut control) = Runtime::new(
        prepared(slots, bytes),
        Limits::for_plan(&prepared(slots, bytes), 8, 8),
    )
    .unwrap()
    .with_sample_updates(queued)
    .unwrap();
    control.set_wake(std::thread::current()).unwrap();
    (rt, control)
}
#[test]
fn sounding_voices_pin_exact_old_rate_window_loop_and_bytes_after_commit() {
    let (mut rt, mut control) = runtime(2, 4096, 1);
    let old = rt.trigger(input(1), 60, 1.).unwrap();
    let mut prefix = [[0.; 2]; 2];
    rt.render(&mut prefix).unwrap();
    assert_eq!(prefix, [[1.; 2], [2.; 2]]);
    let first = Arc::new(SampleTask::default());
    control
        .submit(revision(10., 96000, 20), first.clone())
        .unwrap();
    // Independent old/new runtimes supply the established resampler oracle;
    // the twice-rate source legitimately uses the antialiasing kernel.
    let mut old_reference = Runtime::new(
        prepared(2, 4096),
        Limits::for_plan(&prepared(2, 4096), 8, 8),
    )
    .unwrap();
    old_reference.trigger(input(1), 60, 1.).unwrap();
    old_reference.render(&mut [[0.; 2]; 2]).unwrap();
    let p = Prepared::new(
        48000,
        vec![Pcm::new(96000, (0..20).map(|i| [10. + i as f32; 2]).collect()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 1).unwrap(),
            playback: playback(2, 6),
        }],
        4,
    )
    .unwrap();
    let lim = Limits::for_plan(&p, 8, 8);
    let mut new_reference = Runtime::new(p, lim).unwrap();
    new_reference.trigger(input(1), 60, 1.).unwrap();
    let mut expected = [[0.; 2]; 6];
    let mut old_audio = [[0.; 2]; 6];
    let mut new_audio = [[0.; 2]; 6];
    old_reference.render(&mut old_audio).unwrap();
    new_reference.render(&mut new_audio).unwrap();
    for i in 0..6 {
        expected[i] = [old_audio[i][0] + new_audio[i][0]; 2];
    }
    let mut audio = [[0.; 2]; 6];
    support::without_heap(|| {
        assert_eq!(rt.poll_sample_update(), Ok(true));
    });
    assert!(
        !first.finished(),
        "commit alone is not a control acknowledgement"
    );
    assert!(control.reply().unwrap().result.is_ok());
    assert!(first.success());
    let new = rt.trigger(input(2), 60, 1.).unwrap();
    support::without_heap(|| {
        rt.render(&mut audio).unwrap();
    });
    assert_eq!(
        audio.map(|f| f.map(f32::to_bits)),
        expected.map(|f| f.map(f32::to_bits)),
        "immutable old and twice-rate new sources match independent runtimes bit for bit"
    );
    assert_ne!(rt.note_plan(old), Err(Error::StaleHandle));
    assert_eq!(rt.note_plan(old), rt.note_plan(new));
    // A sounding revised voice prevents its slot being overwritten by a later commit.
    let second = Arc::new(SampleTask::default());
    control
        .submit(revision(20., 48000, 12), second.clone())
        .unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(control.reply().unwrap().result.is_ok());
    let mut after = [[0.; 2]; 4];
    support::without_heap(|| {
        rt.render(&mut after).unwrap();
    });
    let mut old_after = [[0.; 2]; 4];
    let mut new_after = [[0.; 2]; 4];
    old_reference.render(&mut old_after).unwrap();
    new_reference.render(&mut new_after).unwrap();
    let expected_after = std::array::from_fn::<_, 4, _>(|i| [old_after[i][0] + new_after[i][0]; 2]);
    assert_eq!(
        after.map(|f| f.map(f32::to_bits)),
        expected_after.map(|f| f.map(f32::to_bits))
    );
}
#[test]
fn cancellation_slots_bytes_and_reply_pressure_fail_without_source_mutation_or_audio_drops() {
    let (mut rt, mut control) = runtime(1, 256, 1);
    let cancelled = Arc::new(SampleTask::default());
    control
        .submit(revision(10., 48000, 12), cancelled.clone())
        .unwrap();
    assert!(cancelled.cancel());
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert_eq!(control.reply().unwrap().result, Err(Error::StaleHandle));
    assert!(cancelled.cancelled());
    let oversized = Arc::new(SampleTask::default());
    assert_eq!(
        control
            .submit(revision(10., 48000, 33), oversized.clone())
            .unwrap_err()
            .0,
        Error::InvalidInput
    );
    let first = Arc::new(SampleTask::default());
    control
        .submit(revision(10., 48000, 12), first.clone())
        .unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(!first.finished());
    let blocked = Arc::new(SampleTask::default());
    control
        .submit(revision(20., 48000, 12), blocked.clone())
        .unwrap();
    support::without_heap(|| {
        assert_eq!(rt.poll_sample_update(), Err(Error::Capacity));
    });
    assert!(control.reply().unwrap().result.is_ok());
    let held = rt.trigger(input(1), 60, 1.).unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert_eq!(control.reply().unwrap().result, Err(Error::Capacity));
    let mut audio = [[0.; 2]; 3];
    support::without_heap(|| {
        rt.render(&mut audio).unwrap();
    });
    assert_eq!(audio, [12., 13., 14.].map(|f| [f; 2]));
    support::without_heap(|| {
        rt.release_at(held, rt.now()).unwrap();
        rt.render(&mut [[0.; 2]; 1]).unwrap();
    });
    // The same slot can now be reused and the retired PCM leaves only through the reply.
    let reuse = Arc::new(SampleTask::default());
    control
        .submit(revision(20., 48000, 12), reuse.clone())
        .unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    let receipt = control.reply().unwrap();
    assert!(receipt.result.is_ok());
    assert_eq!(receipt.retired.iter().flatten().count(), 1);
}
#[test]
fn distinct_authored_targets_are_not_capped_at_the_retired_reply_batch_size() {
    let pcm = Pcm::new(48000, vec![[0.; 2]; 8].into()).unwrap();
    let regions = (0..128)
        .map(|key| Region {
            sample: 0,
            key_low: key,
            key_high: key,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let plan = Prepared::new(48000, vec![pcm], regions, 128)
        .unwrap()
        .with_sample_targets(
            (0..128)
                .map(|id| SampleTarget {
                    id: SampleTargetId(id),
                    regions: vec![id as usize].into(),
                })
                .collect(),
            SampleRevisionLimits {
                slots: 129,
                resident_bytes: 1 << 20,
            },
        )
        .unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (mut rt, mut control) = Runtime::new(plan, limits)
        .unwrap()
        .with_sample_updates(1)
        .unwrap();
    for id in 0..128 {
        let task = Arc::new(SampleTask::default());
        let pcm = Pcm::new(48000, vec![[id as f32; 2]; 8].into()).unwrap();
        control
            .submit(
                PreparedSampleRevision::new(SampleTargetId(id), pcm, Playback::default(), 48000)
                    .unwrap(),
                task.clone(),
            )
            .unwrap();
        support::without_heap(|| {
            rt.poll_sample_update().unwrap();
        });
        assert!(control.reply().unwrap().result.is_ok());
        assert!(task.success());
    }
    assert_eq!(rt.resident_bytes(), (128 + 1) * 8 * 8);
}

#[test]
fn old_plan_requests_are_rejected_and_all_owned_payloads_retire_only_on_control() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Lifetime(Arc<AtomicUsize>);
    impl Drop for Lifetime {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let plan = prepared(2, 4096);
    let lim = Limits::for_plan(&plan, 8, 8);
    let (rt, mut plans) = Runtime::with_plan_updates(plan, lim, 2, 1).unwrap();
    let (mut rt, mut samples) = rt.with_sample_updates(1).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let task = Arc::new(SampleTask::default());
    samples
        .submit(
            revision(10., 48000, 12).with_lifetime(Lifetime(dropped.clone())),
            task.clone(),
        )
        .unwrap();
    plans.submit(Box::new(prepared(2, 4096))).unwrap();
    support::without_heap(|| {
        rt.poll_plan_update().unwrap();
        rt.poll_sample_update().unwrap();
    });
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    let reply = samples.reply().unwrap();
    assert_eq!(reply.result, Err(Error::StaleHandle));
    assert!(task.finished() && !task.success());
    drop(reply);
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn dormant_targets_report_absent_source_and_unsupported_per_key_pitch_honestly() {
    use sampler_core::{DORMANT_SAMPLE, Rejection};
    let plan = Prepared::new(
        48000,
        vec![],
        vec![Region {
            sample: DORMANT_SAMPLE,
            key_low: 0,
            key_high: 127,
            root_key: Some(60),
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        128,
    )
    .unwrap()
    .with_sample_targets(
        vec![SampleTarget {
            id: SampleTargetId(7),
            regions: vec![0].into(),
        }],
        SampleRevisionLimits {
            slots: 2,
            resident_bytes: 4096,
        },
    )
    .unwrap();
    let lim = Limits::for_plan(&plan, 8, 8);
    let (mut rt, mut control) = Runtime::new(plan, lim)
        .unwrap()
        .with_sample_updates(1)
        .unwrap();
    rt.record_selections(true);
    rt.trigger(input(1), 60, 1.).unwrap();
    assert_eq!(
        rt.take_selection_records()[0].candidates[0].rejected,
        Some(Rejection::SourceUnavailable)
    );
    let task = Arc::new(SampleTask::default());
    control.submit(revision(10., 48000, 12), task).unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(control.reply().unwrap().result.is_ok());
    rt.trigger(input(2), 127, 1.).unwrap();
    assert_eq!(
        rt.take_selection_records()[0].candidates[0].rejected,
        Some(Rejection::SourcePitch)
    );
    rt.trigger(input(3), 60, 1.).unwrap();
    assert_eq!(rt.take_selection_records()[0].candidates[0].rejected, None);
}
