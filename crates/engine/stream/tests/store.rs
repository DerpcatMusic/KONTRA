#![allow(clippy::single_range_in_vec_init)] // pin lists of one window

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kontra_stream::{
    BlobKey, Encoding, MAX_CHANNELS, MemSource, PAGE_FRAMES, SampleFmt, Source, Store, StoreConfig,
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
    sample.prefetch(&client, first, 300, 0, 1.0, false);
    let mut out = vec![[0.0; 2]; 300];
    assert!(eventually(
        || sample.read(&client, first, &mut out).missing == 0
    ));
    for (n, frame) in out.iter().enumerate() {
        assert_eq!(frame[0], value(first + n as u64));
    }
}

#[test]
fn a_run_of_requested_pages_lands_page_by_page() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let (key, source) = ramp(8 * PAGE_FRAMES + 77);
    let sample = store.load(&key, source, 8 * PAGE_FRAMES + 77, &[]).unwrap();

    // Pages 2..=8 (the last one short) asked for at once: served in one read.
    let first = 2 * PAGE_FRAMES;
    let frames = 6 * PAGE_FRAMES + 77;
    sample.prefetch(&client, first, frames, 0, 1.0, false);
    let mut out = vec![[0.0; 2]; frames as usize];
    assert!(eventually(
        || sample.read(&client, first, &mut out).missing == 0
    ));
    for (n, frame) in out.iter().enumerate() {
        assert_eq!(frame[0], value(first + n as u64), "frame {n}");
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
            sample.prefetch(&client, first, PAGE_FRAMES, 0, 1.0, false);
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

/// Two zones of one load naming the same sample (Areia's pads: 640 zones,
/// 320 samples) read it once and hold one copy, its windows merged.
#[test]
fn one_sample_twice_in_a_load_is_read_and_held_once() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let bytes = (0..2 * PAGE_FRAMES)
        .flat_map(|n| (n as i16).to_le_bytes())
        .collect();
    let source = Arc::new(Counting(MemSource::new(FMT, bytes), AtomicUsize::new(0)));
    let key = BlobKey {
        file: "twice".into(),
        offset: 0,
        fmt: FMT,
    };
    let request = |pin| kontra_stream::LoadRequest {
        key: key.clone(),
        source: source.clone(),
        frames: 2 * PAGE_FRAMES,
        pin: vec![pin],
    };
    let loaded = store.load_many(vec![
        request(0..PAGE_FRAMES),
        request(PAGE_FRAMES..2 * PAGE_FRAMES),
    ]);
    let [Ok(a), Ok(b)] = &loaded[..] else {
        panic!("load failed")
    };
    assert!(Arc::ptr_eq(a, b));
    // One read of both pages, both resident for either zone.
    assert_eq!(source.1.load(Ordering::Relaxed), 1);
    let client = store.client();
    let mut out = vec![[0.0; 2]; 10];
    assert_eq!(a.read(&client, PAGE_FRAMES, &mut out).missing, 0);
}

#[test]
fn an_unsupported_layout_is_refused_at_load() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let fmt = SampleFmt {
        encoding: Encoding::I16,
        channels: 17,
    };
    let key = BlobKey {
        file: "f".into(),
        offset: 0,
        fmt,
    };
    let source = Arc::new(MemSource::new(fmt, vec![0; 80]));
    assert!(store.load(&key, source, 10, &[]).is_err());
}

#[test]
fn a_six_channel_sample_pins_and_streams_through_its_mix() {
    let fmt = SampleFmt {
        encoding: Encoding::I16,
        channels: 6,
    };
    // Channel c of frame n holds n + c, so the mix's picks can be checked.
    let frames = 4 * PAGE_FRAMES;
    let bytes = (0..frames)
        .flat_map(|n| (0..6).flat_map(move |c| (((n + c) % 32_768) as i16).to_le_bytes()))
        .collect();
    let key = BlobKey {
        file: "six".into(),
        offset: 0,
        fmt,
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let source = Arc::new(MemSource::new(fmt, bytes));
    let sample = store.load(&key, source, frames, &[0..PAGE_FRAMES]).unwrap();
    let mut mix = [[0.0; 2]; MAX_CHANNELS];
    mix[5] = [1.0, 0.0];
    mix[2] = [0.0, 1.0];
    let mut out = vec![[0.0; 2]; PAGE_FRAMES as usize];
    assert_eq!(
        sample.read_mixed(&client, 0, &mut out, Some(&mix)).missing,
        0
    );
    assert_eq!(out[7], [value(12), value(9)]);
    let late = 3 * PAGE_FRAMES;
    assert!(eventually(|| {
        sample.prefetch(&client, late, PAGE_FRAMES, 0, 1.0, false);
        sample
            .read_mixed(&client, late, &mut out, Some(&mix))
            .missing
            == 0
    }));
    assert_eq!(out[7], [value(late + 12), value(late + 9)]);
}

#[test]
fn an_ncw_head_stays_compressed_and_plays_exactly() {
    let frames = 3000u64;
    let samples: Vec<i32> = (0..frames as i32)
        .flat_map(|n| [n * 40 - 60_000, 1000 - n * 7])
        .collect();
    let spec = ncw::PcmSpec {
        channels: 2,
        bits_per_sample: 24,
        sample_rate: 48_000,
    };
    let data = ncw::encode_pcm(&samples, spec, ncw::StereoMode::Auto).unwrap();
    let path = std::env::temp_dir().join(format!("kontra-stream-{}.ncw", std::process::id()));
    std::fs::write(&path, &data).unwrap();
    let sample = kontra_ir::Sample {
        name: "ncw".into(),
        file: path.clone(),
        offset: 0,
        len: None,
        channels: 2,
        frames,
        rate: 48_000,
        keystream: None,
    };
    let (source, fmt) = kontra_stream::open(&sample).unwrap();
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let key = BlobKey {
        file: "ncw".into(),
        offset: 0,
        fmt,
    };
    let loaded = store.load(&key, source, frames, &[0..frames]).unwrap();
    // Held as stored: under the packed 24-bit PCM it decodes to.
    assert!(
        store.pinned_bytes() < frames * 6 / 2,
        "{}",
        store.pinned_bytes()
    );
    // Read in odd pieces across block and page edges.
    let client = store.client();
    let mut at = 0;
    for len in [1usize, 511, 2, 700, 1000, 786].into_iter().cycle() {
        if at >= frames {
            break;
        }
        let mut out = vec![[9.0; 2]; len];
        assert_eq!(loaded.read(&client, at, &mut out).missing, 0);
        for (n, frame) in out.iter().enumerate() {
            let i = at as usize + n;
            let expect = match samples.get(2 * i..2 * i + 2) {
                Some([l, r]) => [*l as f32 / 8_388_608.0, *r as f32 / 8_388_608.0],
                _ => [0.0; 2],
            };
            assert_eq!(*frame, expect, "frame {i}");
        }
        at += len as u64;
    }
    std::fs::remove_file(path).unwrap();
}

/// A monolith's samples read through one handle on its file: the file is
/// opened once per load, not once per sample.
#[test]
#[cfg(target_os = "linux")]
fn the_samples_of_one_file_open_it_once() {
    let frames = 1000u64;
    let samples: Vec<i32> = (0..frames as i32).flat_map(|n| [n, -n]).collect();
    let spec = ncw::PcmSpec {
        channels: 2,
        bits_per_sample: 24,
        sample_rate: 48_000,
    };
    let one = ncw::encode_pcm(&samples, spec, ncw::StereoMode::Auto).unwrap();
    let path = std::env::temp_dir().join(format!("kontra-monolith-{}.nkx", std::process::id()));
    std::fs::write(&path, [one.as_slice(), one.as_slice()].concat()).unwrap();
    let at = |offset: u64| kontra_ir::Sample {
        name: format!("at {offset}"),
        file: path.clone(),
        offset,
        len: Some(one.len() as u64),
        channels: 2,
        frames,
        rate: 48_000,
        keystream: None,
    };
    let handles = || {
        (std::fs::read_dir("/proc/self/fd").unwrap().flatten())
            .filter(|fd| std::fs::read_link(fd.path()).is_ok_and(|p| p == path))
            .count()
    };
    let files = kontra_stream::Files::new([path.as_path()]);
    let opened = [at(0), at(one.len() as u64)].map(|s| files.open(&s).unwrap());
    drop(files);
    assert_eq!(handles(), 1);
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    for (n, (source, fmt)) in opened.into_iter().enumerate() {
        let key = BlobKey {
            file: format!("monolith {n}").into(),
            offset: 0,
            fmt,
        };
        let loaded = store.load(&key, source, frames, &[0..frames]).unwrap();
        let mut out = vec![[0.0; 2]; 4];
        loaded.read(&store.client(), 500, &mut out);
        assert_eq!(out[1][0], -out[1][1], "sample {n} reads its own bytes");
        assert!(out[1][0] > 0.0);
    }
    std::fs::remove_file(&path).unwrap();
}
