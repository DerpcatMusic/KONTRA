//! Synthetic page-pool RSS and reactivation probe; no library PCM is recorded.
use sampler_core::{Pcm, StreamCache, PageStatus, PageUpdate, PAGE_FRAMES};
use serde_json::json;
use std::time::{Duration, Instant};
#[path = "../tests/support/mod.rs"]
mod support;

fn rss() -> u64 {
    std::fs::read_to_string("/proc/self/status").unwrap().lines()
        .find_map(|line| line.strip_prefix("VmRSS:").map(|v| v.split_whitespace().next().unwrap().parse().unwrap())).unwrap()
}
fn main() {
    const PARTS: usize = 16;
    const PAGES: usize = 768;
    let initial = rss();
    let started = Instant::now();
    let mut parts = (0..PARTS).map(|_| {
        let asset = Pcm::streamed(48000, PAGE_FRAMES * PAGES).unwrap();
        let (cache, worker) = StreamCache::new(PAGES).unwrap();
        (asset, cache, worker)
    }).collect::<Vec<_>>();
    let loaded = rss();
    let setup_ms = started.elapsed().as_secs_f64() * 1000.;
    for (asset, cache, worker) in &mut parts {
        for page in 0..PAGES {
            support::without_heap(|| {
                assert_eq!(cache.request(asset, page, 0), Ok(PageStatus::Pending));
                let mut job = worker.next_job().unwrap();
                job.frames_mut().fill([0.125, -0.25]);
                worker.complete(job, Ok(())).unwrap();
                assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
                assert_eq!(cache.frame(asset.asset_id(), page * PAGE_FRAMES), Some([0.125, -0.25]));
            });
        }
    }
    let warm = rss();
    for (asset, cache, worker) in &mut parts {
        support::without_heap(|| {
            for page in 0..PAGES {
                assert_eq!(cache.invalidate(sampler_core::PageKey { asset: asset.asset_id(), index: page }), Ok(true));
            }
            assert!(worker.next_job().is_none());
        });
    }
    std::thread::sleep(Duration::from_secs(6));
    for (_, _, worker) in &mut parts { assert!(worker.next_job().is_none()); }
    let idle = rss();
    for (asset, cache, worker) in &mut parts {
        support::without_heap(|| {
            assert_eq!(cache.request(asset, 0, 0), Ok(PageStatus::Pending));
            let mut job = worker.next_job().unwrap();
            assert!(job.frames_mut().iter().all(|f| *f == [0.; 2]));
            job.frames_mut().fill([0.375, -0.5]);
            worker.complete(job, Ok(())).unwrap();
            assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
            assert_eq!(cache.frame(asset.asset_id(), 0), Some([0.375, -0.5]));
        });
    }
    println!("{}", json!({"parts":PARTS,"pages_per_part":PAGES,"rss_initial_kib":initial,"rss_loaded_kib":loaded,"rss_warm_kib":warm,"rss_idle_kib":idle,"rss_reactivated_kib":rss(),"setup_ms":setup_ms,"reactivation_pcm_equal":true,"audio_heap_calls":0}));
}
