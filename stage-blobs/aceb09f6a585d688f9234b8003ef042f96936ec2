use super::*;

fn cache_frame(frame: u64) -> Frame {
    [
        (frame % 1021) as f32 / 2048.,
        -((frame % 997) as f32) / 2048.,
    ]
}

fn cache_fixture(path: &std::path::Path) -> Arc<Source> {
    let mut wav = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in 0..CHUNK * (BLOCKS as u64 + 8) + 17 {
        for value in cache_frame(frame) {
            wav.write_sample(value).unwrap();
        }
    }
    wav.finalize().unwrap();
    Arc::new(crate::audio::Sources::default().source(path).unwrap())
}

#[test]
fn decoded_block_buffers_reuse_preserves_pcm_failure_and_eof() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache.wav");
    let source = cache_fixture(&path);
    let mut cache = Decoded::default();
    let mut out = [[0.; 2]; CHUNK as usize];
    for block in 0..=BLOCKS {
        cache
            .read_frames(&source, block as u64 * CHUNK, &mut out)
            .unwrap();
        for (i, frame) in out.iter().enumerate() {
            assert_eq!(*frame, cache_frame(block as u64 * CHUNK + i as u64));
        }
    }
    let buffers: std::collections::HashSet<_> = cache
        .blocks
        .values()
        .map(|(_, frames)| frames.as_ptr())
        .chain(cache.spare.iter().map(|frames| frames.as_ptr()))
        .collect();
    assert_eq!(buffers.len(), BLOCKS + 1);
    for block in BLOCKS + 1..BLOCKS + 8 {
        cache
            .read_frames(&source, block as u64 * CHUNK, &mut out)
            .unwrap();
        assert!(buffers.contains(
            &cache.blocks[&(Arc::as_ptr(&source) as usize, block as u64 * CHUNK)]
                .1
                .as_ptr()
        ));
        for (i, frame) in out.iter().enumerate() {
            assert_eq!(*frame, cache_frame(block as u64 * CHUNK + i as u64));
        }
    }
    let broken_path = dir.path().join("truncated.wav");
    std::fs::copy(&path, &broken_path).unwrap();
    let len = std::fs::metadata(&broken_path).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&broken_path)
        .unwrap()
        .set_len(len - 34 * 8)
        .unwrap();
    let broken = Arc::new(
        crate::audio::Sources::default()
            .source(&broken_path)
            .unwrap(),
    );
    let order = cache.order.clone();
    let spare = cache.spare.as_ref().unwrap().as_ptr();
    assert!(cache
        .read_frames(&broken, (BLOCKS as u64 + 7) * CHUNK, &mut out)
        .is_err());
    assert_eq!(cache.order, order, "failed misses must not evict valid PCM");
    assert_eq!(cache.blocks.len(), BLOCKS);
    assert_eq!(cache.spare.as_ref().unwrap().as_ptr(), spare);
    for ((_, start), (_, frames)) in &cache.blocks {
        for (i, frame) in frames.iter().enumerate() {
            assert_eq!(*frame, cache_frame(*start + i as u64));
        }
    }
    let mut boundary = [[1.; 2]; CHUNK as usize + 37];
    let start = CHUNK * (BLOCKS as u64 + 8) - 19;
    cache.read_frames(&source, start, &mut boundary).unwrap();
    for (i, frame) in boundary.iter().enumerate() {
        assert_eq!(
            *frame,
            if i < 36 {
                cache_frame(start + i as u64)
            } else {
                [0.; 2]
            }
        );
    }
    assert!(cache
        .blocks
        .values()
        .all(|(_, frames)| buffers.contains(&frames.as_ptr())));
    cache.clear();
    assert!(cache.blocks.is_empty() && cache.readers.is_empty() && cache.spare.is_none());
}

/// Frozen ce94add cache-miss path for a same-binary release comparison.
fn allocating_read(
    cache: &mut Decoded,
    source: &Arc<Source>,
    mut start: u64,
    mut out: &mut [Frame],
) -> anyhow::Result<()> {
    let id = Arc::as_ptr(source) as usize;
    while !out.is_empty() {
        let base = start / CHUNK * CHUNK;
        let key = (id, base);
        if !cache.blocks.contains_key(&key) {
            cache.clock = cache.clock.wrapping_add(1);
            if !cache.readers.contains_key(&id) {
                let reader = source.open_stream()?;
                if cache.readers.len() == READERS {
                    let oldest = *cache.readers.iter().min_by_key(|(_, r)| r.2).unwrap().0;
                    cache.readers.remove(&oldest);
                }
                cache
                    .readers
                    .insert(id, (source.clone(), reader, cache.clock));
            }
            let (_, reader, used) = cache.readers.get_mut(&id).unwrap();
            *used = cache.clock;
            if base >= reader.frames {
                out.fill([0.; 2]);
                return Ok(());
            }
            let mut frames = vec![[0.0; 2]; CHUNK as usize].into_boxed_slice();
            reader.read(base, &mut frames)?;
            cache.decodes += 1;
            if cache.blocks.len() == BLOCKS {
                cache.blocks.remove(&cache.order.pop_front().unwrap());
            }
            cache.blocks.insert(key, (source.clone(), frames));
            cache.order.push_back(key);
        }
        let offset = (start - base) as usize;
        let n = out.len().min(CHUNK as usize - offset);
        out[..n].copy_from_slice(&cache.blocks[&key].1[offset..offset + n]);
        out = &mut out[n..];
        start += n as u64;
    }
    Ok(())
}

#[test]
#[ignore = "release-only decoded cache-miss benchmark"]
fn decoded_block_buffer_reuse_speed() {
    const ROUNDS: usize = 64;
    const READS: usize = (BLOCKS + 8) * ROUNDS;
    let dir = tempfile::tempdir().unwrap();
    let source = cache_fixture(&dir.path().join("cache-speed.wav"));
    let mut medians = [Vec::new(), Vec::new()];
    let hasher = std::collections::hash_map::RandomState::new();
    let run = |reuse: bool, count: bool| {
        let mut cache = Decoded {
            blocks: HashMap::with_hasher(hasher.clone()),
            ..Decoded::default()
        };
        let mut out = [[0.; 2]; 64];
        let read = |cache: &mut Decoded, start, out: &mut [Frame]| {
            if reuse {
                cache.read_frames(&source, start, out)
            } else {
                allocating_read(cache, &source, start, out)
            }
            .unwrap();
        };
        for block in 0..=BLOCKS {
            read(&mut cache, block as u64 * CHUNK, &mut out);
        }
        let decodes = cache.decodes;
        let start = Instant::now();
        let mut work = || {
            for block in 0..READS {
                read(&mut cache, (block % (BLOCKS + 8)) as u64 * CHUNK, &mut out);
                std::hint::black_box(&out);
            }
        };
        let allocations = if count {
            crate::plugin::tests::allocations(work)
        } else {
            work();
            0
        };
        let elapsed = start.elapsed();
        assert_eq!(cache.decodes - decodes, READS);
        assert_eq!(cache.blocks.len(), BLOCKS);
        (elapsed.as_secs_f64(), allocations)
    };
    for trial in 0..7 {
        for reuse in if trial % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let (seconds, _) = run(reuse, false);
            medians[usize::from(reuse)].push(seconds);
        }
    }
    for values in &mut medians {
        values.sort_by(f64::total_cmp);
    }
    let before = medians[0][3];
    let after = medians[1][3];
    let (_, allocating) = run(false, true);
    let (_, reused) = run(true, true);
    assert_eq!(
        allocating - reused,
        2 * READS,
        "one PCM allocation and free per successful cache miss are removed"
    );
    println!("decoded-cache-miss {READS} reads: allocating median={before:.6}s reuse median={after:.6}s delta={:.2}% heap before={allocating} after={reused}", (after / before - 1.) * 100.);
}
