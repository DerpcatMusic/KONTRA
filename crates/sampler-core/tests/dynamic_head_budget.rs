//! Immutable revision ownership and physical streamed-head quota are separate
//! from logical arena/queue counters. Replies retain the physical payload.
mod support;
use sampler_core::{
    Envelope, Error, HeadBudget, Input, Limits, Loop, LoopMode, LoopShape, Pcm, Playback, Prepared,
    PreparedSampleRevision, Protocol, Region, Runtime, SampleControl, SampleRevisionLimits,
    SampleTarget, SampleTargetId, SampleTask, StreamCache,
};
use std::sync::Arc;
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
            resident_bytes: 32,
        },
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 2);
    let (cache, _worker) = StreamCache::new(8).unwrap();
    Runtime::new(plan, limits)
        .unwrap()
        .with_stream_cache(cache)
        .with_sample_updates(1)
        .unwrap()
}
fn revision(budget: &HeadBudget, value: f32) -> PreparedSampleRevision {
    let pcm = Pcm::streamed(48000, 16).unwrap();
    pcm.bind_head_budget(budget).unwrap();
    let mut builder = pcm.head_preparer().builder();
    builder.push(0, &[[value; 2]; 8]).unwrap();
    pcm.publish_ranges(builder.finish()).unwrap();
    PreparedSampleRevision::new(SampleTargetId(7), pcm, playback(), 48000).unwrap()
}
fn assert_full(budget: &HeadBudget) {
    let pcm = Pcm::streamed(48000, 16).unwrap();
    pcm.bind_head_budget(budget).unwrap();
    let mut builder = pcm.head_preparer().builder();
    support::without_heap(|| assert_eq!(builder.push(0, &[[0.25; 2]; 8]), Err(Error::Capacity)));
}
#[test]
fn old_voices_and_retired_replies_keep_physical_quota_until_control_drops_the_payload() {
    let (mut rt, mut control) = player();
    let budget = control.head_budget();
    assert_eq!(rt.sample_head_budget_status().unwrap(), budget.status());
    control
        .submit(revision(&budget, 0.25), Arc::new(SampleTask::default()))
        .unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    assert!(control.reply().unwrap().result.is_ok());
    let old = rt.trigger(input(1), 60, 1.).unwrap();
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
    assert_eq!(
        audio, [[0.75; 2]; 7],
        "old and future starts retain their own immutable source bytes"
    );
    assert_eq!(rt.voice_count(), 2);
    assert_eq!(budget.status().held_bytes, 32);
    assert_eq!(rt.sample_head_budget_status().unwrap(), budget.status());
    assert_full(&budget);
    support::without_heap(|| {
        rt.release_at(old, rt.now()).unwrap();
        rt.render(&mut [[0.; 2]; 1]).unwrap();
    });
    assert_eq!(rt.voice_count(), 1);
    control.collect().unwrap();
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    let retained = control.reply().unwrap();
    assert_eq!(retained.retired.iter().flatten().count(), 1);
    assert_eq!(
        budget.status().held_bytes,
        32,
        "logical arena removal cannot release a returned payload"
    );
    assert_full(&budget);
    drop(retained);
    assert_eq!(budget.status().held_bytes, 16);
    let ready = revision(&budget, 0.75);
    assert_eq!(budget.status().held_bytes, 32);
    assert_eq!(rt.sample_head_budget_status().unwrap(), budget.status());
    drop(ready);
    assert_eq!(budget.status().held_bytes, 16);
    assert_eq!(budget.status().peak_bytes, 32);
    drop(rt);
    assert_eq!(budget.status().held_bytes, 0);
}
#[test]
fn canceled_pending_payload_is_not_freed_on_audio_or_by_logical_queue_acknowledgement() {
    let (mut rt, mut control) = player();
    let budget = control.head_budget();
    assert_eq!(rt.sample_head_budget_status().unwrap(), budget.status());
    let task = Arc::new(SampleTask::default());
    control
        .submit(revision(&budget, 0.25), task.clone())
        .unwrap();
    assert!(task.cancel());
    support::without_heap(|| {
        rt.poll_sample_update().unwrap();
    });
    let reply = control.reply().unwrap();
    assert_eq!(reply.result, Err(Error::StaleHandle));
    assert_eq!(budget.status().held_bytes, 16);
    assert!(task.cancelled());
    drop(reply);
    assert_eq!(budget.status().held_bytes, 0);
}

#[test]
fn a_foreign_streamed_revision_cannot_bypass_client_physical_admission() {
    let (rt, mut control) = player();
    let foreign = HeadBudget::new(32);
    let rejected = control
        .submit(revision(&foreign, 0.25), Arc::new(SampleTask::default()))
        .unwrap_err();
    assert_eq!(rejected.0, Error::RevisionConflict);
    assert_eq!(foreign.status().held_bytes, 16);
    assert_eq!(rt.sample_head_budget_status().unwrap().held_bytes, 0);
    drop(rejected);
    assert_eq!(foreign.status().held_bytes, 0);
}
