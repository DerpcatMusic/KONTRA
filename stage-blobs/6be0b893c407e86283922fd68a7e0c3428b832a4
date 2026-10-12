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
