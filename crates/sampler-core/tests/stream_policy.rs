use sampler_core::*;
mod support;

fn player(pages: usize) -> (Runtime, StreamWorker) {
    let pcm = Pcm::headed(48000, PAGE_FRAMES, &[[0.25; 2]; 32]).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![Region {
        sample: 0, key_low: 60, key_high: 60, root_key: None,
        velocity_low: 0., velocity_high: 1., gain: 1.,
        envelope: Envelope::default(), playback: Playback::default(),
    }], 1).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, worker) = StreamCache::new(pages).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    rt.trigger(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
    (rt, worker)
}

#[test]
fn service_retries_a_transient_decode_without_audio_heap_work() {
    let (mut rt, mut worker) = player(4);
    support::without_heap(|| {
        assert_eq!(rt.service_streaming(64), Ok(false));
        let job = worker.next_job().unwrap();
        worker.complete(job, Err(DecodeFailure::Unavailable)).unwrap();
        assert_eq!(rt.service_streaming(64), Ok(false));
        assert!(worker.next_job().is_none(), "backoff prevents a hot retry loop");
    });
    std::thread::sleep(std::time::Duration::from_millis(20));
    support::without_heap(|| {
        assert_eq!(rt.service_streaming(64), Ok(false));
        let mut job = worker.next_job().expect("demand must retry a transient decode");
        job.frames_mut().fill([0.25; 2]);
        worker.complete(job, Ok(())).unwrap();
        assert_eq!(rt.service_streaming(64), Ok(true));
        let mut out = [[0.; 2]; 64];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[0.25; 2]; 64]);
        assert_eq!(rt.stream_underruns(), 0);
    });
}

#[test]
fn corrupt_decodes_are_reported_without_retrying_forever() {
    let (mut rt, mut worker) = player(4);
    rt.service_streaming(64).unwrap();
    let job = worker.next_job().unwrap();
    worker.complete(job, Err(DecodeFailure::InvalidSamples)).unwrap();
    support::without_heap(|| {
        assert!(rt.service_streaming(64).is_err(), "corruption must be observable");
        assert!(worker.next_job().is_none());
    });
}

#[test]
fn offline_readiness_is_bounded_and_disconnect_is_visible() {
    let (mut rt, worker) = player(4);
    support::without_heap(|| {
        assert_eq!(rt.wait_streaming(64, std::time::Duration::from_millis(2)), Err(StreamError::Timeout));
    });
    drop(worker);
    support::without_heap(|| {
        assert_eq!(rt.wait_streaming(64, std::time::Duration::from_millis(2)), Err(StreamError::Disconnected));
    });
}

#[test]
fn offline_waits_after_delayed_starts_and_script_pitch_changes() {
    fn runtime(streamed: bool) -> (Runtime, Option<StreamWorker>) {
        let pcm = if streamed { Pcm::headed(48000, PAGE_FRAMES * 3, &[[0.25; 2]; 32]).unwrap() }
            else { Pcm::new(48000, vec![[0.25; 2]; PAGE_FRAMES * 3].into_boxed_slice()).unwrap() };
        let plan = Prepared::new(48000, vec![pcm], vec![], 0).unwrap();
        let limits = Limits::for_plan(&plan, 8, 8);
        let mut rt = Runtime::new(plan, limits).unwrap();
        let worker = if streamed {
            let (cache, worker) = StreamCache::new(8).unwrap();
            rt = rt.with_stream_cache(cache);
            Some(worker)
        } else { None };
        rt.set_offline(true);
        rt.set_cold_starts(true);
        let note = rt.note_on(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
        rt.start(note, 0, 17, 1.).unwrap();
        rt.set_note_param(note, ModTarget::Pitch, 48., false).unwrap();
        (rt, worker)
    }
    let (mut reference, _) = runtime(false);
    let (mut streamed, worker) = runtime(true);
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop = done.clone();
    let decoder = std::thread::spawn(move || {
        let mut worker = worker.unwrap();
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(mut job) = worker.next_job() {
                std::thread::sleep(std::time::Duration::from_millis(5));
                job.frames_mut().fill([0.25; 2]);
                worker.complete(job, Ok(())).unwrap();
            } else { std::thread::sleep(std::time::Duration::from_micros(100)); }
        }
    });
    let mut expected = [[0.; 2]; 256];
    reference.render(&mut expected).unwrap();
    let mut got = [[0.; 2]; 256];
    support::without_heap(|| streamed.render(&mut got).unwrap());
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    decoder.join().unwrap();
    assert!(got == expected, "delayed onset and changed pitch must preserve every frame");
    assert_eq!(streamed.stream_underruns(), 0);
}

#[test]
fn storage_capacity_refuses_an_incompatible_voice_without_heap_or_live_eviction() {
    let a = Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 32]).unwrap();
    let b = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
    let plan = Prepared::new(48000, vec![a, b], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, mut worker) = StreamCache::new(1).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    let live = rt.start(note, 0, 0, 1.).unwrap();
    assert_eq!(rt.service_streaming(64), Ok(false));
    let mut job = worker.next_job().unwrap();
    job.frames_mut().fill([0.25; 2]);
    worker.complete(job, Ok(())).unwrap();
    assert_eq!(rt.service_streaming(64), Ok(true));
    rt.render(&mut [[0.; 2]; 64]).unwrap();
    let second = rt.note_on(input(2), 60, 1.).unwrap();
    support::without_heap(|| {
        assert_eq!(rt.start(second, 1, rt.now(), 1.), Err(Error::Capacity));
        assert!(rt.voice_active(live));
        assert_eq!(rt.voice_count(), 1);
        rt.stop_voice(live).unwrap();
        rt.start(second, 1, rt.now(), 1.).unwrap();
        assert_eq!(rt.voice_count(), 1);
    });
}

#[test]
fn storage_admission_precedes_stealing_and_counts_refusals() {
    let pcm = || Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 32]).unwrap();
    let plan = Prepared::new(48000, vec![pcm(), pcm()], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 1);
    let (cache, mut worker) = StreamCache::new(1).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    rt.set_stream_horizon(64).unwrap();
    rt.set_voice_stealing(Some(Stealing::for_limits(48000, 1))).unwrap();
    let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
    let first = rt.note_on(input(1), 60, 1.).unwrap();
    let live = rt.start(first, 0, 0, 1.).unwrap();
    rt.service_streaming(64).unwrap();
    let mut job = worker.next_job().unwrap();
    job.frames_mut().fill([0.25; 2]);
    worker.complete(job, Ok(())).unwrap();
    rt.service_streaming(64).unwrap();
    let second = rt.note_on(input(2), 60, 1.).unwrap();
    support::without_heap(|| {
        assert_eq!(rt.start(second, 1, 0, 1.), Err(Error::Capacity));
        assert!(rt.voice_active(live));
        assert_eq!(rt.steals(), 0);
        assert_eq!(rt.stats().stream_capacity_refusals, 1);
    });
}

#[test]
fn admission_accounts_for_pitch_and_crossfade_source_windows() {
    let pcm = Pcm::headed(48000, PAGE_FRAMES * 8, &[[0.25; 2]; 32]).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, _worker) = StreamCache::new(1).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    rt.set_stream_horizon(1024).unwrap();
    let note = rt.note_on(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
    let family = rt.create_family(note).unwrap();
    support::without_heap(|| {
        assert_eq!(rt.start_family(family, 0, 0, 1., Envelope::default(), Playback { transpose_semitones: 48., ..Default::default() }), Err(Error::Capacity));
        assert_eq!(rt.voice_count(), 0);
        assert_eq!(rt.start_family(family, 0, 0, 1., Envelope::default(), Playback {
            start: PAGE_FRAMES - 512,
            loop_range: Some(Loop { start: PAGE_FRAMES, end: PAGE_FRAMES * 2, mode: LoopMode::Continuous, shape: LoopShape::Crossfade { frames: 512 }, passes: None }),
            ..Default::default()
        }), Err(Error::Capacity));
        assert_eq!(rt.voice_count(), 0);
    });
}

#[test]
fn shared_storage_budget_allows_more_than_256_distinct_live_sources() {
    let assets = (0..300).map(|_| Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 32]).unwrap()).collect();
    let plan = Prepared::new(48000, assets, vec![], 0).unwrap();
    let limits = Limits { families: 512, ..Limits::for_plan(&plan, 512, 512) };
    let (cache, _worker) = StreamCache::new(900).unwrap();
    assert_eq!(cache.voice_budget(), 300);
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_stream_horizon(64).unwrap();
    support::without_heap(|| {
        for sample in 0..300 {
            let note = rt.note_on(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(sample as i32) }, 60, 1.).unwrap();
            rt.start(note, sample, 0, 1.).unwrap();
        }
        assert_eq!(rt.voice_count(), 300);
        assert_eq!(rt.stats().stream_capacity_refusals, 0);
    });
}

#[test]
fn residency_reads_and_last_reader_release_never_touch_the_audio_heap() {
    let snapshot = std::sync::Arc::new(sampler_pool::Snapshot::new(vec![[0.25; 2]; 64]));
    let old = snapshot.read();
    let control = snapshot.clone();
    std::thread::spawn(move || control.replace(vec![[0.5; 2]; 64], |_| ())).join().unwrap();
    support::without_heap(|| {
        assert_eq!(&**snapshot.read(), &[[0.5; 2]; 64]);
        assert_eq!(&**old, &[[0.25; 2]; 64]);
        drop(old);
    });
    snapshot.collect();
}

#[test]
fn starting_a_voice_reserves_capacity_without_page_walks_or_decoder_jobs() {
    let pcm = Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 32]).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, mut worker) = StreamCache::new(2).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_stream_horizon(64).unwrap();
    let note = rt.note_on(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
    support::without_heap(|| rt.start(note, 0, 0, 1.).map(|_| ()).unwrap());
    assert!(worker.next_job().is_none(), "start must not run page service or wake decoders");
    rt.service_streaming(64).unwrap();
    assert!(worker.next_job().is_some(), "normal block service issues the page request");
}

#[test]
fn advancing_a_live_cursor_updates_credits_before_the_next_start() {
    let a = Pcm::headed(48000, PAGE_FRAMES * 3, &[[0.25; 2]; PAGE_FRAMES]).unwrap();
    let b = Pcm::streamed(48000, PAGE_FRAMES * 3).unwrap();
    let plan = Prepared::new(48000, vec![a, b], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, _worker) = StreamCache::new(2).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    rt.set_stream_horizon(64).unwrap();
    let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    let live = rt.start(note, 0, 0, 1.).unwrap();
    let second = rt.note_on(input(2), 60, 1.).unwrap();
    support::without_heap(|| {
        rt.render(&mut [[0.; 2]; PAGE_FRAMES - 32]).unwrap();
        assert_eq!(rt.start(second, 1, rt.now(), 1.), Err(Error::Capacity), "live cursor now needs two pages, leaving no credit for a third");
        assert!(rt.voice_active(live));
        rt.stop_voice(live).unwrap();
        rt.start(second, 1, rt.now(), 1.).unwrap();
    });
}

#[test]
fn a_pitch_edit_updates_live_credits_before_the_next_start() {
    let a = Pcm::headed(48000, PAGE_FRAMES * 3, &[[0.25; 2]; 32]).unwrap();
    let b = Pcm::streamed(48000, PAGE_FRAMES * 3).unwrap();
    let plan = Prepared::new(48000, vec![a, b], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, _worker) = StreamCache::new(2).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    rt.set_stream_horizon(1024).unwrap();
    let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    let live = rt.start(note, 0, 0, 1.).unwrap();
    let expression = rt.expression_id(note).unwrap();
    let second = rt.note_on(input(2), 60, 1.).unwrap();
    support::without_heap(|| {
        rt.set_expression(expression, Expression { pitch_semitones: 24., ..Default::default() }).unwrap();
        assert_eq!(rt.start(second, 1, 0, 1.), Err(Error::Capacity));
        assert!(rt.voice_active(live));
        rt.stop_voice(live).unwrap();
        rt.start(second, 1, 0, 1.).unwrap();
    });
}

#[test]
fn a_horizon_change_updates_live_credits_before_the_next_start() {
    let pcm = Pcm::headed(48000, PAGE_FRAMES * 3, &[[0.25; 2]; PAGE_FRAMES]).unwrap();
    let short = Pcm::headed(48000, PAGE_FRAMES, &[[0.25; 2]; 64]).unwrap();
    let plan = Prepared::new(48000, vec![pcm, short], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    let (cache, _worker) = StreamCache::new(2).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_stream_horizon(64).unwrap();
    let input = |id| Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) };
    let first = rt.note_on(input(1), 60, 1.).unwrap();
    let live = rt.start(first, 0, 0, 1.).unwrap();
    rt.render(&mut [[0.; 2]; 64]).unwrap();
    let second = rt.note_on(input(2), 60, 1.).unwrap();
    support::without_heap(|| {
        rt.set_stream_horizon(PAGE_FRAMES as u32 * 2).unwrap();
        assert_eq!(rt.start(second, 1, rt.now(), 1.), Err(Error::Capacity));
        assert!(rt.voice_active(live));
    });
}
