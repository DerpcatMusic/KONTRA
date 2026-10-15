//! Numeric, source-free witness tests. No native-equivalence or host claim.
mod support;
use sampler_core::*;
use sampler_core::trace::onset::{Record, Reader, Birth};

fn input(id: i32) -> Input {
    Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(id) }
}
fn region(sample: usize, base: usize) -> Region {
    Region { sample, key_low: 60, key_high: 60, root_key: None, velocity_low: 0.,
        velocity_high: 1., gain: 1., envelope: Envelope::default(),
        playback: Playback { start: base, ..Default::default() } }
}
fn resident(traced: bool, capacity: usize) -> Runtime {
    let pcm = Pcm::new(48000, (0..256).map(|i| if i == 75 { [0.25, -0.5] } else { [0.; 2] })
        .collect::<Vec<_>>().into_boxed_slice()).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![region(0, 7)], 1).unwrap()
        .with_voice_modulation(vec![ModProgram { sources: vec![ModSource::Controller(113)],
            routes: vec![ModRoute::new(0, ModTarget::SampleStart, 1.)], ..Default::default() }],
            vec![Some(0)], vec![32]).unwrap().with_initial_controllers(&[(113, 127)]);
    let limits = Limits::for_plan(&plan, 4, 4);
    Runtime::new(if traced { plan.with_signal_trace(capacity).unwrap() } else { plan }, limits).unwrap()
}
fn births(reader: &Reader) -> Vec<Birth> {
    assert_eq!(reader.dropped(), 0);
    reader.drain().unwrap().into_iter().filter_map(|r| match r { Record::Birth(b) => Some(b), _ => None }).collect()
}

#[test]
fn actual_dynamic_offset_is_distinct_from_base_and_traced_pcm_matches() {
    let (mut on, mut off) = (resident(true, 8192), resident(false, 0));
    let signal = on.signal_trace_reader().unwrap();
    let onset = signal.onset_reader();
    let mut a = [[0.; 2]; 128];
    let mut b = a;
    support::without_heap(|| {
        on.trigger(input(1), 60, 1.).unwrap();
        off.trigger(input(1), 60, 1.).unwrap();
        on.render(&mut a).unwrap(); off.render(&mut b).unwrap();
    });
    assert_eq!(a, b, "instrumented empty-chain insertion must preserve this fixture's PCM");
    let first = births(&onset);
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].cursor.base_start, first[0].used_start_offset, first[0].cursor.frame), (7, 32, Some(39)));
    assert_eq!(first[0].cursor.fraction, 0.);
    assert!(!first[0].cold);
    let rows = signal.drain();
    assert_eq!(signal.dropped(), 0);
    let contributions: Vec<_> = rows.iter().filter(|r| r.contribution).collect();
    assert!(!contributions.is_empty());
    assert!(contributions.iter().all(|r| r.identity.voice == Some(first[0].voice)));
    let first_source = contributions.iter().filter_map(|r|
        r.identity.source_metrics.onset.nonzero[0].map(|i| r.at + u64::from(i))).min();
    assert_eq!(first_source, Some(36));
    let performance = on.performance(0).unwrap();
    let before = on.now();
    support::without_heap(|| {
        on.set_controller(performance, 113, 0).unwrap();
        on.trigger(input(2), 60, 1.).unwrap();
    });
    let second = births(&onset);
    assert_eq!(second.len(), 1);
    assert_eq!((second[0].at, second[0].used_start_offset, second[0].cursor.frame), (before, 0, Some(7)));
    assert_ne!(first[0].voice, second[0].voice);
}

fn streamed(traced: bool) -> (Runtime, StreamWorker, [Pcm; 2]) {
    let assets = [Pcm::streamed(48000, PAGE_FRAMES).unwrap(), Pcm::streamed(48000, PAGE_FRAMES).unwrap()];
    let plan = Prepared::new(48000, assets.to_vec(), vec![region(0, 0), region(1, 0)], 2).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let (cache, worker) = StreamCache::new(8).unwrap();
    let mut rt = Runtime::new(if traced { plan.with_signal_trace(8192).unwrap() } else { plan }, limits)
        .unwrap().with_stream_cache(cache);
    rt.set_stream_horizon(128).unwrap(); rt.set_cold_starts(true);
    (rt, worker, assets)
}
fn complete(worker: &mut StreamWorker, mut job: DecodeJob, channel: usize) {
    let start = job.range().start;
    for (i, frame) in job.frames_mut().iter_mut().enumerate() {
        *frame = [0.; 2];
        if start + i == 0 { frame[channel] = 1.; }
    }
    worker.complete(job, Ok(())).unwrap();
}
fn held_capture(traced: bool) -> (Vec<Frame>, Vec<Birth>, [Option<u64>; 2]) {
    let (mut rt, mut worker, assets) = streamed(traced);
    let signal = rt.signal_trace_reader();
    let onset = signal.as_ref().map(|s| s.onset_reader());
    support::without_heap(|| { rt.trigger(input(1), 60, 1.).unwrap(); rt.service_streaming(128).unwrap(); });
    let mut jobs = [None, None];
    while let Some(job) = worker.next_job() {
        let sample = assets.iter().position(|p| p.asset_id() == job.key().asset).unwrap();
        assert!(jobs[sample].is_none()); jobs[sample] = Some(job);
    }
    complete(&mut worker, jobs[0].take().unwrap(), 0);
    support::without_heap(|| { rt.service_streaming(128).unwrap(); });
    let records = onset.as_ref().map(|reader| {
        assert_eq!(reader.dropped(), 0);
        reader.drain().unwrap()
    }).unwrap_or_default();
    let born: Vec<_> = records.iter().filter_map(|r| match r { Record::Birth(b) => Some(*b), _ => None }).collect();
    if traced {
        assert_eq!(born.len(), 2);
        assert_eq!(born[0].event, born[1].event);
        assert_eq!(born[0].family, born[1].family);
        assert_ne!(born[0].voice, born[1].voice);
        assert_ne!(born[0].asset, born[1].asset);
        assert!(born.iter().all(|b| b.cold && b.cursor.frame == Some(0)));
        let windows: Vec<_> = records.iter().filter_map(|r| match r { Record::Window(w) => Some(w), _ => None }).collect();
        assert_eq!(windows.len(), 2);
        for window in windows {
            assert_eq!((window.at, window.page, window.frames), (0, 0, [0, 1]));
            assert!(!window.resident);
            assert_eq!(window.page_status, 0, "birth precedes service's initial page requests");
            assert!(born.iter().any(|b| b.voice == window.voice && b.asset == window.asset));
        }
    }
    let mut output = vec![[0.; 2]; 256];
    support::without_heap(|| rt.render(&mut output[..128]).unwrap());
    let mut snapshots = Vec::new();
    assert!(rt.visit_onset_voices(|s| { snapshots.push(s); true }));
    assert!(snapshots.iter().any(|s| s.asset == assets[1].asset_id() && s.cold && s.cursor.frame == Some(0)));
    complete(&mut worker, jobs[1].take().unwrap(), 1);
    support::without_heap(|| { rt.service_streaming(128).unwrap(); rt.render(&mut output[128..]).unwrap(); });
    let mut first = [None; 2];
    if let Some(signal) = signal {
        assert_eq!(signal.dropped(), 0);
        for row in signal.drain().into_iter().filter(|r| r.contribution) {
            for channel in 0..2 {
                if let Some(i) = row.output.onset.nonzero[channel] {
                    let at = row.at + u64::from(i);
                    first[channel] = Some(first[channel].map_or(at, |old: u64| old.min(at)));
                }
            }
        }
        assert_eq!(onset.unwrap().dropped(), 0);
    }
    assert_eq!(rt.stream_underruns(), 0);
    (output, born, first)
}

#[test]
fn two_assets_one_event_staggered_worker_pages_have_sample_accurate_onsets() {
    let (traced, births, onsets) = held_capture(true);
    let (plain, _, _) = held_capture(false);
    assert_eq!(traced, plain);
    assert_eq!(onsets, [Some(0), Some(128)]);
    assert_eq!(births.len(), 2);
    assert_eq!(traced.iter().position(|f| f[0] != 0.), Some(0));
    assert_eq!(traced.iter().position(|f| f[1] != 0.), Some(128));
}

#[test]
fn bounded_birth_queue_reports_loss_without_callback_heap_use() {
    // A fresh one-row trace; constructor and queue ownership stay off audio.
    let assets = vec![Pcm::new(48000, vec![[0.25; 2]; 128].into_boxed_slice()).unwrap()];
    let plan = Prepared::new(48000, assets, vec![region(0, 0), region(0, 0)], 2).unwrap().with_signal_trace(1).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let reader = rt.signal_trace_reader().unwrap().onset_reader();
    support::without_heap(|| { rt.trigger(input(1), 60, 1.).unwrap(); });
    assert_eq!(reader.drain().unwrap().len(), 1);
    assert_eq!(reader.dropped(), 3, "two births and two first windows, capacity one");
}

#[test]
fn onset_reduction_keeps_subthreshold_nonzero_separate() {
    let pcm = Pcm::new(48000, vec![[0., 0.], [1e-7, 0.], [2e-5, 0.], [0., -2e-5]].into_boxed_slice()).unwrap();
    let plan = Prepared::new(48000, vec![pcm], vec![region(0, 0)], 1).unwrap().with_signal_trace(128).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let reader = rt.signal_trace_reader().unwrap();
    support::without_heap(|| { rt.trigger(input(1), 60, 1.).unwrap(); rt.render(&mut [[0.; 2]; 4]).unwrap(); });
    let source = reader.drain().into_iter().find(|r| r.contribution).unwrap().identity.source_metrics;
    assert_eq!(source.onset.nonzero, [Some(1), Some(3)]);
    assert_eq!(source.onset.above_threshold, [Some(2), Some(3)]);
}

#[test]
fn reused_physical_slot_retains_a_new_generation() {
    let mut rt = resident(true, 8192);
    let reader = rt.signal_trace_reader().unwrap().onset_reader();
    support::without_heap(|| {
        rt.trigger(input(1), 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 256]).unwrap();
    });
    let first = births(&reader)[0];
    assert_eq!(rt.voice_count(), 0);
    support::without_heap(|| { rt.trigger(input(2), 60, 1.).unwrap(); });
    let next = births(&reader)[0];
    assert_eq!(next.voice.slot, first.voice.slot);
    assert_ne!(next.voice.generation, first.voice.generation);
    assert_ne!(next.voice, first.voice);
    assert_eq!(next.at, 256);
    let mut current = None;
    assert!(rt.visit_onset_voices(|s| { assert!(current.is_none()); current = Some(s.voice); true }));
    assert_eq!(current, Some(next.voice));
}

#[test]
fn whole_view_wavetable_demand_is_bounded_independently_of_ring_capacity() {
    for capacity in [1, 1024] {
    let asset = Pcm::streamed(48000, PAGE_FRAMES * 128).unwrap();
    let mut wave = region(0, 0);
    wave.playback.wavetable = Some(Wavetable::default());
    let plan = Prepared::new(48000, vec![asset], vec![wave], 1).unwrap()
        .with_signal_trace(capacity).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let (cache, _worker) = StreamCache::new(256).unwrap();
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    let reader = rt.signal_trace_reader().unwrap().onset_reader();
    support::without_heap(|| { rt.trigger(input(1), 60, 1.).unwrap(); });
    let rows = reader.drain().unwrap();
    assert_eq!(rows.iter().filter(|r| matches!(r, Record::Birth(_))).count(), 1);
    assert_eq!(rows.iter().filter(|r| matches!(r, Record::Window(_))).count(),
        if capacity == 1 { 0 } else { sampler_core::trace::onset::MAX_WINDOW_ROWS });
    assert_eq!(reader.dropped(), 1, "first omitted attempt invalidates the incomplete witness");
    }
}
