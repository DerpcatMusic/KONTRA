//! Page-index fill/churn cost. Synthetic PCM stays in RAM; decoding is not timed.
use sampler_core::{PAGE_FRAMES, PageStatus, PageUpdate, Pcm, StreamCache, StreamWorker};
use std::{hint::black_box, time::Instant};
#[path = "../tests/support/mod.rs"]
mod support;

fn complete(cache: &mut StreamCache, worker: &mut StreamWorker) {
    let mut job = worker.next_job().unwrap();
    let key = job.key();
    job.frames_mut().fill([0.25; 2]);
    worker.complete(job, Ok(())).unwrap();
    assert_eq!(cache.poll(), Some(PageUpdate::Loaded(key)));
    assert_eq!(cache.status(key), PageStatus::Ready);
}

fn report(pages: usize, phase: &str, times: &mut [u128]) {
    times.sort_unstable();
    println!("{pages},{phase},{:.3},{:.3},0", times[times.len() / 2] as f64 / 1000., times[(times.len() - 1) * 99 / 100] as f64 / 1000.);
}

fn main() {
    assert!(!cfg!(debug_assertions), "run with --profile corpus or --release");
    println!("pages,phase,median_us,p99_us,heap_calls");
    for pages in [768, 6144] {
        let pcm = Pcm::streamed(48000, PAGE_FRAMES * pages * 3).unwrap();
        let (mut cache, mut worker) = StreamCache::new(pages).unwrap();
        let mut times = vec![0; pages];
        for page in (0..pages).rev() {
            support::without_heap(|| {
                let start = Instant::now();
                assert_eq!(black_box(cache.request(&pcm, page, 0)), Ok(PageStatus::Pending));
                times[page] = start.elapsed().as_nanos();
                complete(&mut cache, &mut worker);
            });
        }
        report(pages, "fill", &mut times);
        for page in 0..pages {
            support::without_heap(|| {
                cache.begin_epoch().unwrap();
                let start = Instant::now();
                assert_eq!(black_box(cache.request(&pcm, pages + page, 0)), Ok(PageStatus::Pending));
                times[page] = start.elapsed().as_nanos();
                complete(&mut cache, &mut worker);
            });
        }
        report(pages, "churn", &mut times);
    }
}
