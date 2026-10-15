//! Keep aggregate admission diagnostics while distinguishing the resource.
use sampler_core::*;
mod support;

fn player(pcm: Pcm, voices: usize, pages: Option<usize>) -> (Runtime, Option<StreamWorker>, [NoteId; 2]) {
    let plan = Prepared::new(48000, vec![pcm], vec![], 0).unwrap();
    let limits = Limits::for_plan(&plan, 8, voices);
    let rt = Runtime::new(plan, limits).unwrap();
    let (mut rt, worker) = if let Some(n) = pages {
        let (cache, worker) = StreamCache::new(n).unwrap();
        (rt.with_stream_cache(cache), Some(worker))
    } else { (rt, None) };
    rt.set_stream_horizon(64).unwrap();
    rt.set_cold_starts(true);
    let notes = std::array::from_fn(|id| rt.note_on(Input {
        protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60,
        external_id: Some(id as i32 + 1),
    }, 60, 1.).unwrap());
    (rt, worker, notes)
}

#[test]
fn full_voice_slots_and_stream_pages_have_separate_refusal_reasons() {
    for streamed in [false, true] {
        let pcm = if streamed {
            Pcm::headed(48000, PAGE_FRAMES * 2, &[[0.25; 2]; 32]).unwrap()
        } else { Pcm::new(48000, vec![[0.25; 2]; 128].into_boxed_slice()).unwrap() };
        let (mut rt, _worker, [first, second]) = player(pcm, if streamed { 8 } else { 1 }, streamed.then_some(1));
        support::without_heap(|| {
            let voice = rt.start(first, 0, 0, 1.).unwrap();
            assert_eq!(rt.start(second, 0, 0, 1.), Err(Error::Capacity));
            assert!(rt.voice_active(voice));
            let stats = rt.stats();
            assert_eq!(stats.voice_drops, 1);
            assert_eq!(stats.voice_pool_refusals, u64::from(!streamed));
            assert_eq!(stats.stream_capacity_refusals, u64::from(streamed));
            assert_eq!(stats.waiting_voice_replacements, 0);
        });
    }
}

#[test]
fn silent_waiting_victim_replacement_is_counted_without_a_refusal() {
    let pcm = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
    let (mut rt, _worker, [first, second]) = player(pcm, 1, Some(4));
    support::without_heap(|| {
        let old = rt.start(first, 0, 0, 1.).unwrap();
        let new = rt.start(second, 0, 0, 1.).unwrap();
        assert!(!rt.voice_active(old));
        assert!(rt.voice_active(new));
        let stats = rt.stats();
        assert_eq!(stats.cold_starts, 2);
        assert_eq!(stats.waiting_voice_replacements, 1);
        assert_eq!(stats.voice_pool_refusals, 0);
        assert_eq!(stats.stream_capacity_refusals, 0);
        assert_eq!(stats.voice_drops, 0);
        assert_eq!(stats.voices, 1);
    });
}

#[test]
fn stream_refusal_preserves_waiting_victim_and_aggregate_count() {
    let pcm = Pcm::streamed(48000, PAGE_FRAMES * 2).unwrap();
    let (mut rt, _worker, [first, second]) = player(pcm, 1, Some(1));
    support::without_heap(|| {
        let old = rt.start(first, 0, 0, 1.).unwrap();
        assert_eq!(rt.start(second, 0, 0, 1.), Err(Error::Capacity));
        assert!(rt.voice_active(old));
        let stats = rt.stats();
        assert_eq!(stats.cold_starts, 1);
        assert_eq!(stats.waiting_voice_replacements, 0);
        assert_eq!(stats.voice_pool_refusals, 0);
        assert_eq!(stats.stream_capacity_refusals, 1);
        assert_eq!(stats.voice_drops, 1);
    });
}
