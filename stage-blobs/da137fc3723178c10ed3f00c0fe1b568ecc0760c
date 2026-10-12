use sampler_core::*;
mod support;

fn asset() -> Pcm {
    Pcm::new(
        48000,
        (0..PAGE_FRAMES * 2 + 3)
            .map(|i| [i as f32, -(i as f32)])
            .collect(),
    )
    .unwrap()
}
fn key(asset: &Pcm, index: usize) -> PageKey {
    PageKey {
        asset: asset.asset_id(),
        index,
    }
}
fn decode(worker: &mut StreamWorker, asset: &Pcm) -> (PageKey, *const Frame) {
    let mut job = worker.next_job().unwrap();
    assert_eq!(job.key().asset, asset.asset_id());
    let range = job.range();
    let pointer = job.frames_mut().as_ptr();
    job.frames_mut()
        .copy_from_slice(&asset.resident_frames().unwrap()[range]);
    let key = job.key();
    worker.complete(job, Ok(())).unwrap();
    (key, pointer)
}

#[test]
fn decoded_pages_share_buffers_across_views_and_protected_pages_survive_eviction() {
    let asset = asset();
    let alias = asset.clone();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    support::without_heap(|| {
        assert_eq!(cache.request(&asset, 0, 10), Ok(PageStatus::Pending));
        assert_eq!(cache.request(&alias, 0, 10), Ok(PageStatus::Pending));
        assert_eq!(cache.request(&asset, 1, 20), Ok(PageStatus::Pending));
        assert_eq!(cache.request(&asset, 2, 0), Err(StreamError::Capacity));
        let (first, first_pointer) = decode(&mut worker, &asset);
        assert_eq!(first, key(&asset, 0));
        let (second, second_pointer) = decode(&mut worker, &asset);
        assert_eq!(second, key(&asset, 1));
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(first)));
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(second)));
        assert_eq!(
            cache
                .span(asset.asset_id(), 0..PAGE_FRAMES)
                .unwrap()
                .as_ptr(),
            first_pointer
        );
        assert_eq!(
            cache
                .span(asset.asset_id(), PAGE_FRAMES..2 * PAGE_FRAMES)
                .unwrap()
                .as_ptr(),
            second_pointer
        );
        assert_eq!(
            cache.frame(alias.asset_id(), PAGE_FRAMES + 5),
            Some([(PAGE_FRAMES + 5) as f32, -((PAGE_FRAMES + 5) as f32)])
        );
        assert!(
            cache
                .span(asset.asset_id(), PAGE_FRAMES - 1..PAGE_FRAMES + 1)
                .is_none()
        );
        cache.begin_epoch().unwrap();
        assert_eq!(cache.protect(&alias, 0..PAGE_FRAMES), Ok(true));
        assert_eq!(cache.request(&asset, 2, 30), Ok(PageStatus::Pending));
        assert_eq!(cache.status(second), PageStatus::Missing);
        assert_eq!(cache.status(first), PageStatus::Ready);
        let (last, reused) = decode(&mut worker, &asset);
        assert_eq!(last, key(&asset, 2));
        assert_eq!(reused, second_pointer);
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(last)));
        assert_eq!(
            cache
                .span(asset.asset_id(), 2 * PAGE_FRAMES..2 * PAGE_FRAMES + 3)
                .unwrap(),
            &asset.resident_frames().unwrap()[2 * PAGE_FRAMES..]
        );
        assert!(cache.frame(asset.asset_id(), asset.frame_count()).is_none());
        assert_eq!(cache.request(&asset, 3, 0), Err(StreamError::InvalidRange));
        assert_eq!(
            cache.request(&asset, usize::MAX, 0),
            Err(StreamError::InvalidRange)
        );
        assert_eq!(
            cache.protect(&asset, 0..asset.frame_count() + 1),
            Err(StreamError::InvalidRange)
        );
    });
}

#[test]
fn worker_coalesces_priority_changes_and_launches_each_request_once() {
    let asset = asset();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    support::without_heap(|| {
        cache.request(&asset, 0, 20).unwrap();
        cache.request(&asset, 1, 10).unwrap();
        assert_eq!(cache.request(&asset, 0, 5), Err(StreamError::Capacity));
        let first = worker.next_job().unwrap();
        assert_eq!((first.key(), first.deadline()), (key(&asset, 1), 10));
        cache.request(&asset, 0, 5).unwrap();
        cache.request(&asset, 0, 3).unwrap();
        let second = worker.next_job().unwrap();
        assert_eq!((second.key(), second.deadline()), (key(&asset, 0), 3));
        cache.request(&asset, 0, 1).unwrap(); // Already dispatched: no duplicate job.
        assert!(worker.next_job().is_none());
        worker
            .complete(first, Err(DecodeFailure::Unavailable))
            .unwrap();
        worker
            .complete(second, Err(DecodeFailure::Unavailable))
            .unwrap();
        assert!(matches!(
            cache.poll(),
            Some(PageUpdate::Failed(_, DecodeFailure::Unavailable))
        ));
        assert!(matches!(
            cache.poll(),
            Some(PageUpdate::Failed(_, DecodeFailure::Unavailable))
        ));
        assert!(worker.next_job().is_none());
        assert_eq!(
            cache.request(&asset, 0, 0),
            Ok(PageStatus::Failed(DecodeFailure::Unavailable))
        );
    });
}

#[test]
fn priority_rebuilds_and_stale_jobs_do_not_allocate_or_duplicate_work() {
    let asset = Pcm::streamed(48000, PAGE_FRAMES * 4).unwrap();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    support::without_heap(|| {
        cache.request(&asset, 0, 0).unwrap();
        cache.request(&asset, 1, 0).unwrap();
        let first = worker.next_job().unwrap();
        let second = worker.next_job().unwrap();
        cache.begin_epoch().unwrap();
        cache.request(&asset, 2, 200).unwrap();
        cache.request(&asset, 3, 100).unwrap();
        assert!(worker.next_job().is_none());
        for deadline in (0..200).rev() {
            cache.request(&asset, 2, deadline).unwrap();
            assert!(worker.next_job().is_none());
        }
        worker.complete(first, Ok(())).unwrap();
        worker.complete(second, Ok(())).unwrap();
        assert!(matches!(cache.poll(), Some(PageUpdate::Discarded(_))));
        assert!(matches!(cache.poll(), Some(PageUpdate::Discarded(_))));
        let urgent = worker.next_job().unwrap();
        let later = worker.next_job().unwrap();
        assert_eq!((urgent.key(), urgent.deadline()), (key(&asset, 2), 0));
        assert_eq!((later.key(), later.deadline()), (key(&asset, 3), 100));
        worker.complete(urgent, Ok(())).unwrap();
        worker.complete(later, Ok(())).unwrap();
        assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
        assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
        assert!(worker.next_job().is_none());
    });
}

#[test]
fn late_completion_cannot_overwrite_a_reused_slot_and_returns_its_buffer() {
    let asset = asset();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    support::without_heap(|| {
        cache.request(&asset, 0, 0).unwrap();
        let mut obsolete = worker.next_job().unwrap();
        obsolete.frames_mut().fill([0.5; 2]);
        cache.begin_epoch().unwrap();
        cache.request(&asset, 1, 20).unwrap();
        cache.begin_epoch().unwrap();
        assert_eq!(
            cache.protect(&asset, PAGE_FRAMES..PAGE_FRAMES + 1),
            Ok(false)
        );
        cache.request(&asset, 2, 1).unwrap();
        let replacement = decode(&mut worker, &asset).0;
        assert_eq!(replacement, key(&asset, 2));
        worker.complete(obsolete, Ok(())).unwrap();
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(replacement)));
        assert_eq!(cache.poll(), Some(PageUpdate::Discarded(key(&asset, 0))));
        assert_eq!(cache.status(key(&asset, 0)), PageStatus::Missing);
        assert_eq!(
            cache.frame(asset.asset_id(), 2 * PAGE_FRAMES),
            Some(asset.resident_frames().unwrap()[2 * PAGE_FRAMES])
        );
        assert_eq!(decode(&mut worker, &asset).0, key(&asset, 1));
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(key(&asset, 1))));
        assert!(worker.next_job().is_none());
    });
}

#[test]
fn failed_or_foreign_decodes_preserve_pool_ownership_and_never_publish_bad_samples() {
    let asset = asset();
    let (mut cache, mut worker) = StreamCache::new(1).unwrap();
    let (_other_cache, mut other) = StreamCache::new(1).unwrap();
    support::without_heap(|| {
        cache.request(&asset, 0, 0).unwrap();
        let mut job = worker.next_job().unwrap();
        let rejected = other.complete(job, Ok(())).unwrap_err();
        assert_eq!(rejected.reason, StreamError::WrongWorker);
        job = rejected.job;
        job.frames_mut()[0][0] = f32::NAN;
        worker.complete(job, Ok(())).unwrap();
        assert_eq!(
            cache.poll(),
            Some(PageUpdate::Failed(
                key(&asset, 0),
                DecodeFailure::InvalidSamples
            ))
        );
        assert!(cache.frame(asset.asset_id(), 0).is_none());
        assert_eq!(cache.invalidate(key(&asset, 0)), Ok(true));
        assert_eq!(cache.invalidate(key(&asset, 0)), Ok(false));
        cache.request(&asset, 0, 0).unwrap();
        let mut job = worker.next_job().unwrap();
        assert!(job.frames_mut().iter().all(|frame| *frame == [0.; 2]));
        let range = job.range();
        job.frames_mut()
            .copy_from_slice(&asset.resident_frames().unwrap()[range]);
        worker.complete(job, Ok(())).unwrap();
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(key(&asset, 0))));
    });
}

#[test]
fn worker_disconnect_keeps_resident_data_but_rejects_unserviceable_requests() {
    let asset = asset();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    cache.request(&asset, 0, 0).unwrap();
    decode(&mut worker, &asset);
    cache.poll();
    cache.request(&asset, 1, 1).unwrap();
    drop(worker); // Endpoint destruction is explicitly off audio.
    support::without_heap(|| {
        assert_eq!(cache.request(&asset, 0, 0), Ok(PageStatus::Ready));
        assert_eq!(cache.request(&asset, 1, 1), Err(StreamError::Disconnected));
        assert_eq!(cache.request(&asset, 2, 1), Err(StreamError::Disconnected));
        assert_eq!(
            cache.frame(asset.asset_id(), 1),
            Some(asset.resident_frames().unwrap()[1])
        );
    });
}

#[test]
fn worker_thread_publishes_owned_pages_that_survive_endpoint_shutdown() {
    let asset = asset();
    let worker_asset = asset.clone();
    let (mut cache, mut worker) = StreamCache::new(2).unwrap();
    support::without_heap(|| {
        cache.request(&asset, 0, 0).unwrap();
    });
    let thread = std::thread::spawn(move || {
        let mut job = worker.next_job().unwrap();
        let range = job.range();
        job.frames_mut()
            .copy_from_slice(&worker_asset.resident_frames().unwrap()[range]);
        worker.complete(job, Ok(())).unwrap();
        // Worker endpoint/free buffers are destroyed on this worker thread.
    });
    thread.join().unwrap();
    support::without_heap(|| {
        assert_eq!(cache.poll(), Some(PageUpdate::Loaded(key(&asset, 0))));
        assert_eq!(
            cache.span(asset.asset_id(), 0..PAGE_FRAMES).unwrap(),
            &asset.resident_frames().unwrap()[..PAGE_FRAMES]
        );
        assert_eq!(cache.request(&asset, 0, 0), Ok(PageStatus::Ready));
        assert_eq!(cache.request(&asset, 1, 0), Err(StreamError::Disconnected));
    });
}
