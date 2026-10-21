mod support;
use sampler_core::{Error, HeadBudget, Pcm, ResidentBuffer};

use sampler_core::{
    Envelope, Input, Limits, Loop, LoopMode, LoopShape, Playback, Prepared, PreparedSampleRevision,
    Protocol, Region, Runtime, SampleControl, SampleRevisionLimits, SampleTarget, SampleTargetId,
    SampleTask,
};
use std::sync::Arc;
fn playback() -> Playback {
    Playback {
        start: 2,
        end: Some(8),
        loop_range: Some(Loop {
            start: 2,
            end: 6,
            shape: LoopShape::Wrap,
            mode: LoopMode::Continuous,
            passes: None,
        }),
        ..Default::default()
    }
}
fn player() -> (Runtime, SampleControl) {
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[0.; 2]; 16].into()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 0).unwrap(),
            playback: playback(),
        }],
        1,
    )
    .unwrap()
    .with_sample_targets(
        vec![SampleTarget {
            id: SampleTargetId(7),
            regions: vec![0].into(),
        }],
        SampleRevisionLimits {
            slots: 3,
            resident_bytes: 256,
        },
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 2);
    Runtime::new(plan, limits)
        .unwrap()
        .with_sample_updates(1)
        .unwrap()
}
fn revision(budget: &HeadBudget, value: f32) -> PreparedSampleRevision {
    let mut buffer = ResidentBuffer::new(48000, 16, budget).unwrap();
    buffer.frames_mut().fill([value; 2]);
    PreparedSampleRevision::new(
        SampleTargetId(7),
        buffer.finish().unwrap(),
        playback(),
        48000,
    )
    .unwrap()
}
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

#[test]
fn full_pcm_reservation_precedes_allocation_and_failed_decode_releases_it() {
    for frames in [1, 8, 4097] {
        let bytes = frames * size_of::<sampler_core::Frame>();
        let short = HeadBudget::new(bytes - 1);
        support::without_heap(|| {
            assert_eq!(
                ResidentBuffer::new(48000, frames, &short).unwrap_err(),
                Error::Capacity
            );
        });
        assert_eq!(short.status().held_bytes, 0);
        assert_eq!(short.status().peak_bytes, 0);
        let budget = HeadBudget::new(bytes);
        let mut buffer = ResidentBuffer::new(48000, frames, &budget).unwrap();
        assert_eq!(budget.status().held_bytes, bytes);
        buffer.frames_mut().fill([0.25, -0.0]);
        buffer.frames_mut()[0][0] = f32::NAN;
        assert_eq!(buffer.finish().unwrap_err(), Error::InvalidInput);
        assert_eq!(budget.status().held_bytes, 0);
        assert_eq!(budget.status().peak_bytes, bytes);
        let staging = ResidentBuffer::new(48000, frames, &budget).unwrap();
        drop(staging); // A decoder/source error or cancellation owns the same staging drop.
        assert_eq!(budget.status().held_bytes, 0);
    }
    let budget = HeadBudget::new(usize::MAX);
    support::without_heap(|| {
        assert_eq!(
            ResidentBuffer::new(0, 1, &budget).unwrap_err(),
            Error::InvalidInput
        );
        assert_eq!(
            ResidentBuffer::new(48000, 0, &budget).unwrap_err(),
            Error::InvalidInput
        );
        assert_eq!(
            ResidentBuffer::new(48000, usize::MAX, &budget).unwrap_err(),
            Error::Capacity
        );
    });
    assert_eq!(budget.status().held_bytes, 0);
}

#[test]
fn partial_heads_and_full_buffers_share_one_physical_quota_until_the_last_owner_drops() {
    let budget = HeadBudget::new(80);
    let headed = Pcm::streamed(48000, 4096).unwrap();
    headed.bind_head_budget(&budget).unwrap();
    let mut builder = headed.head_preparer().builder();
    builder.push(0, &[[0.5; 2]; 8]).unwrap(); // 16 exact mono16 bytes.
    let staged_head = builder.finish();
    let mut buffer = ResidentBuffer::new(44100, 8, &budget).unwrap();
    buffer.frames_mut().fill([0.25, -0.0]);
    assert_eq!(budget.status().held_bytes, 80);
    let pcm = buffer.finish().unwrap();
    headed.publish_ranges(staged_head).unwrap();
    assert_eq!(pcm.sample_rate(), 44100);
    let clone = pcm.clone();
    drop(pcm);
    assert_eq!(budget.status().held_bytes, 80);
    support::without_heap(|| {
        let frames = clone.resident_frames().unwrap();
        assert!(
            frames
                .iter()
                .all(|f| f[0].to_bits() == 0.25f32.to_bits()
                    && f[1].to_bits() == (-0.0f32).to_bits())
        );
        drop(frames);
        assert_eq!(
            ResidentBuffer::new(48000, 1, &budget).unwrap_err(),
            Error::Capacity
        );
    });
    drop(clone);
    assert_eq!(budget.status().held_bytes, 16);
    drop(headed);
    assert_eq!(budget.status().held_bytes, 0);
    assert_eq!(budget.status().peak_bytes, 80);
}

#[test]
fn full_revision_old_voice_and_returned_payload_keep_actual_storage_charged() {
    let (mut rt, mut control) = player();
    let budget = control.physical_budget();
    control
        .submit(revision(&budget, 0.25), Arc::new(SampleTask::default()))
        .unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(control.reply().unwrap().result.is_ok());
    let old = rt.trigger(input(1), 60, 1.).unwrap();
    let old_source = rt.sample_target_asset(SampleTargetId(7)).unwrap().clone();
    control
        .submit(revision(&budget, 0.5), Arc::new(SampleTask::default()))
        .unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(control.reply().unwrap().result.is_ok());
    rt.trigger(input(2), 60, 1.).unwrap();
    let mut audio = [[0.; 2]; 7];
    support::without_heap(|| rt.render(&mut audio).unwrap());
    assert_eq!(audio, [[0.75; 2]; 7]);
    assert_eq!(budget.status().held_bytes, 256);
    support::without_heap(|| {
        assert_eq!(
            ResidentBuffer::new(48000, 1, &budget).unwrap_err(),
            Error::Capacity
        );
        rt.release_at(old, rt.now()).unwrap();
        rt.render(&mut [[0.; 2]; 1]).unwrap();
    });
    control.collect().unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    let reply = control.reply().unwrap();
    assert_eq!(reply.retired.iter().flatten().count(), 1);
    assert_eq!(budget.status().held_bytes, 256);
    drop(reply);
    assert_eq!(
        budget.status().held_bytes,
        256,
        "escaped immutable Pcm still owns its actual buffer"
    );
    drop(old_source);
    assert_eq!(budget.status().held_bytes, 128);
    assert_eq!(rt.sample_physical_budget_status().unwrap(), budget.status());
    drop(rt);
    assert_eq!(budget.status().held_bytes, 0);
}

#[test]
fn canceled_full_payload_and_foreign_authority_cannot_release_or_bypass_admission() {
    let (mut rt, mut control) = player();
    let budget = control.physical_budget();
    let foreign = HeadBudget::new(256);
    let rejected = control
        .submit(revision(&foreign, 0.25), Arc::new(SampleTask::default()))
        .unwrap_err();
    assert_eq!(rejected.0, Error::RevisionConflict);
    assert_eq!(budget.status().held_bytes, 0);
    assert_eq!(foreign.status().held_bytes, 128);
    drop(rejected);
    assert_eq!(foreign.status().held_bytes, 0);
    let task = Arc::new(SampleTask::default());
    control
        .submit(revision(&budget, 0.5), task.clone())
        .unwrap();
    assert!(task.cancel());
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    let reply = control.reply().unwrap();
    assert_eq!(reply.result, Err(Error::StaleHandle));
    assert_eq!(budget.status().held_bytes, 128);
    drop(reply);
    assert_eq!(budget.status().held_bytes, 0);
    // Existing caller-owned PCM is adopted at submission without copying.
    let unbound = Pcm::new(48000, vec![[0.75; 2]; 16].into()).unwrap();
    let id = unbound.asset_id();
    control
        .submit(
            PreparedSampleRevision::new(SampleTargetId(7), unbound, playback(), 48000).unwrap(),
            Arc::new(SampleTask::default()),
        )
        .unwrap();
    assert_eq!(budget.status().held_bytes, 128);
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(control.reply().unwrap().result.is_ok());
    assert_eq!(
        rt.sample_target_asset(SampleTargetId(7))
            .unwrap()
            .asset_id(),
        id
    );
    drop(rt);
    assert_eq!(budget.status().held_bytes, 0);
}

#[test]
fn initial_resident_descriptor_validates_metadata_authority_and_one_shot_publication() {
    let budget = HeadBudget::new(256);
    let pcm = Pcm::pending_resident(44100, 16).unwrap();
    assert!(pcm.requires_resident() && pcm.resident_frames().is_none());
    pcm.bind_resident_budget(&budget).unwrap();
    assert_eq!(
        pcm.publish_initial_resident(ResidentBuffer::new(48000, 16, &budget).unwrap()),
        Err(Error::InvalidInput)
    );
    assert_eq!(budget.status().held_bytes, 0);
    let foreign = HeadBudget::new(256);
    assert_eq!(
        pcm.publish_initial_resident(ResidentBuffer::new(44100, 16, &foreign).unwrap()),
        Err(Error::RevisionConflict)
    );
    assert_eq!(foreign.status().held_bytes, 0);
    let mut data = ResidentBuffer::new(44100, 16, &budget).unwrap();
    data.frames_mut().fill([0.25, -0.5]);
    pcm.publish_initial_resident(data).unwrap();
    assert_eq!(&*pcm.resident_frames().unwrap(), &[[0.25, -0.5]; 16]);
    assert_eq!(budget.status().held_bytes, 128);
    assert_eq!(pcm.resident_non_head_bytes_outside(&budget), 0);
    assert_eq!(pcm.resident_non_head_bytes_outside(&foreign), 128);
    assert_eq!(
        pcm.publish_initial_resident(ResidentBuffer::new(44100, 16, &budget).unwrap()),
        Err(Error::RevisionConflict)
    );
    assert_eq!(&*pcm.resident_frames().unwrap(), &[[0.25, -0.5]; 16]);
    assert_eq!(budget.status().held_bytes, 128);
    drop(pcm);
    assert_eq!(budget.status().held_bytes, 0);
}
