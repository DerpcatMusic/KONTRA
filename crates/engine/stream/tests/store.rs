#![allow(clippy::single_range_in_vec_init)] // pin lists of one window

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kontra_stream::{
    BlobKey, Encoding, MemSource, PAGE_FRAMES, SampleFmt, Source, Store, StoreConfig,
};

const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::I16,
    channels: 1,
};

/// A mono ramp whose frame n holds n (mod 32768), so any read can be checked by position.
fn ramp(frames: u64) -> (BlobKey, Arc<MemSource>) {
    let bytes = (0..frames)
        .flat_map(|n| ((n % 32_768) as i16).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "ramp".into(),
        offset: frames,
        fmt: FMT,
    };
    (key, Arc::new(MemSource::new(FMT, bytes)))
}

fn value(frame: u64) -> f32 {
    (frame % 32_768) as f32 / 32_768.0
}

fn eventually(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    false
}

#[test]
fn a_pinned_head_plays_at_once_and_the_rest_is_silent_but_keeps_time() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let (key, source) = ramp(4 * PAGE_FRAMES);
    let sample = store
        .load(&key, source, 4 * PAGE_FRAMES, &[0..PAGE_FRAMES])
        .unwrap();

    let mut out = vec![[1.0; 2]; 2 * PAGE_FRAMES as usize];
    let read = sample.read(&client, 0, &mut out);
    assert_eq!(read.missing, PAGE_FRAMES as u32);
    for (n, frame) in out.iter().enumerate().take(PAGE_FRAMES as usize) {
        assert_eq!(frame[0], value(n as u64));
    }
    assert!(out[PAGE_FRAMES as usize..].iter().all(|f| *f == [0.0; 2]));
}

#[test]
fn a_prefetched_page_becomes_resident_and_reads_exactly() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let (key, source) = ramp(8 * PAGE_FRAMES);
    let sample = store.load(&key, source, 8 * PAGE_FRAMES, &[]).unwrap();

    let first = 5 * PAGE_FRAMES + 100;
    sample.prefetch(&client, first, 300, 0);
    let mut out = vec![[0.0; 2]; 300];
    assert!(eventually(
        || sample.read(&client, first, &mut out).missing == 0
    ));
    for (n, frame) in out.iter().enumerate() {
        assert_eq!(frame[0], value(first + n as u64));
    }
}

#[test]
fn reads_past_the_end_are_silence_not_missing() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let (key, source) = ramp(100);
    let sample = store.load(&key, source, 100, &[0..100]).unwrap();
    let mut out = vec![[1.0; 2]; 200];
    assert_eq!(sample.read(&client, 0, &mut out).missing, 0);
    assert_eq!(out[99][0], value(99));
    assert!(out[100..].iter().all(|f| *f == [0.0; 2]));
}

#[test]
fn a_small_cache_evicts_and_reloads_without_wrong_audio() {
    // Two slots for a ten-page sample: every page is evicted and read again.
    let config = StoreConfig {
        cache_bytes: 2 * PAGE_FRAMES as usize * 8,
        ..StoreConfig::DEFAULT
    };
    let mut store = Store::new(config).unwrap();
    let client = store.client();
    let (key, source) = ramp(10 * PAGE_FRAMES);
    let sample = store.load(&key, source, 10 * PAGE_FRAMES, &[]).unwrap();
    let mut out = vec![[0.0; 2]; PAGE_FRAMES as usize];
    for round in 0..3 {
        for page in 0..10 {
            let first = page * PAGE_FRAMES;
            sample.prefetch(&client, first, PAGE_FRAMES, 0);
            assert!(
                eventually(|| sample.read(&client, first, &mut out).missing == 0),
                "round {round} page {page}"
            );
            assert_eq!(out[7][0], value(first + 7));
        }
    }
    assert!(client.stats().pages_read >= 30);
}

struct Counting(MemSource, AtomicUsize);

impl Source for Counting {
    fn read(&self, first: u64, out: &mut [u8]) -> std::io::Result<()> {
        self.1.fetch_add(1, Ordering::Relaxed);
        self.0.read(first, out)
    }
}

#[test]
fn two_loads_of_one_blob_share_pinned_pages() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let bytes = (0..2 * PAGE_FRAMES)
        .flat_map(|n| (n as i16).to_le_bytes())
        .collect();
    let source = Arc::new(Counting(MemSource::new(FMT, bytes), AtomicUsize::new(0)));
    let key = BlobKey {
        file: "shared".into(),
        offset: 0,
        fmt: FMT,
    };
    let a = store
        .load(&key, source.clone(), 2 * PAGE_FRAMES, &[0..PAGE_FRAMES])
        .unwrap();
    let b = store
        .load(&key, source.clone(), 2 * PAGE_FRAMES, &[0..2 * PAGE_FRAMES])
        .unwrap();
    // The second load reads only the page the first did not pin.
    assert_eq!(source.1.load(Ordering::Relaxed), 2);
    let client = store.client();
    let mut out = vec![[0.0; 2]; 10];
    assert_eq!(b.read(&client, PAGE_FRAMES, &mut out).missing, 0);
    assert_eq!(out[9][0], value(PAGE_FRAMES + 9));
    drop(a);
}

#[test]
fn an_unsupported_layout_is_refused_at_load() {
    let mut store = Store::new(StoreConfig {
        max_frame_bytes: 4,
        ..StoreConfig::DEFAULT
    })
    .unwrap();
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 2,
    };
    let key = BlobKey {
        file: "f".into(),
        offset: 0,
        fmt,
    };
    let source = Arc::new(MemSource::new(fmt, vec![0; 80]));
    assert!(store.load(&key, source, 10, &[]).is_err());
}
