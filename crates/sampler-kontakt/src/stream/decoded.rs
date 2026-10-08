//! Port from v1 0cb7a8a0:src/engine/stream.rs (Decoded::read_frames).
use super::{Arc, AssetId, AssetSource, DecodeFailure, Frame, HashMap, SampleReader, io};
use std::collections::VecDeque;

const READERS: usize = 64;
const BLOCKS: usize = 128;
const CHUNK: usize = 2048;

#[derive(Default)]
pub(super) struct Decoded {
    readers: HashMap<AssetId, (Arc<dyn AssetSource>, SampleReader, u64)>,
    blocks: HashMap<(AssetId, usize), (Arc<dyn AssetSource>, Box<[Frame]>)>,
    order: VecDeque<(AssetId, usize)>,
    clock: u64,
}

fn failure(error: io::Error) -> DecodeFailure {
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => DecodeFailure::InvalidSamples,
        _ => DecodeFailure::Unavailable,
    }
}

impl Decoded {
    pub(super) fn frame(
        &mut self,
        id: AssetId,
        source: &Arc<dyn AssetSource>,
        start: usize,
    ) -> Result<Frame, DecodeFailure> {
        let mut out = [[0.; 2]];
        self.read(id, source, start, &mut out)?;
        Ok(out[0])
    }

    pub(super) fn read(
        &mut self,
        id: AssetId,
        source: &Arc<dyn AssetSource>,
        mut start: usize,
        mut out: &mut [Frame],
    ) -> Result<(), DecodeFailure> {
        while !out.is_empty() {
            let base = start / CHUNK * CHUNK;
            let key = (id, base);
            if !self.blocks.contains_key(&key) {
                self.clock = self.clock.wrapping_add(1);
                if !self.readers.contains_key(&id) {
                    let reader = source.open_stream().map_err(failure)?;
                    if self.readers.len() == READERS {
                        let oldest = *self.readers.iter().min_by_key(|(_, r)| r.2).unwrap().0;
                        self.readers.remove(&oldest);
                    }
                    self.readers
                        .insert(id, (source.clone(), reader, self.clock));
                }
                let (_, reader, used) = self.readers.get_mut(&id).unwrap();
                *used = self.clock;
                let mut frames = vec![[0.; 2]; CHUNK].into_boxed_slice();
                // v1 zero-fills the final partial block; v2 readers require an exact in-range read.
                let n = reader.frames().saturating_sub(base).min(CHUNK);
                if let Err(error) = if n == 0 {
                    Ok(())
                } else {
                    reader.read(base, &mut frames[..n])
                } {
                    let failure = failure(error);
                    if failure == DecodeFailure::Unavailable {
                        self.readers.remove(&id);
                    }
                    return Err(failure);
                }
                if !frames[..n].iter().flatten().all(|f| f.is_finite()) {
                    return Err(DecodeFailure::InvalidSamples);
                }
                if self.blocks.len() == BLOCKS {
                    self.blocks.remove(&self.order.pop_front().unwrap());
                }
                self.blocks.insert(key, (source.clone(), frames));
                self.order.push_back(key);
            }
            let offset = start - base;
            let n = out.len().min(CHUNK - offset);
            out[..n].copy_from_slice(&self.blocks[&key].1[offset..offset + n]);
            out = &mut out[n..];
            start += n;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::Pcm;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Source {
        frames: usize,
        reads: Arc<AtomicUsize>,
        opens: Arc<AtomicUsize>,
        fail: bool,
    }
    impl AssetSource for Source {
        fn open(&self) -> io::Result<SampleReader> {
            self.opens.fetch_add(1, Ordering::Relaxed);
            let (reads, fail) = (self.reads.clone(), self.fail);
            Ok(SampleReader::custom(
                48000,
                self.frames,
                move |start, out| {
                    if reads.fetch_add(1, Ordering::Relaxed) == 0 && fail {
                        return Err(io::ErrorKind::Interrupted.into());
                    }
                    for (i, f) in out.iter_mut().enumerate() {
                        *f = [(start + i) as f32; 2];
                    }
                    Ok(())
                },
            ))
        }
    }
    fn source(
        frames: usize,
        fail: bool,
    ) -> (
        AssetId,
        Arc<dyn AssetSource>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let reads = Arc::new(AtomicUsize::new(0));
        let opens = Arc::new(AtomicUsize::new(0));
        (
            Pcm::streamed(48000, frames).unwrap().asset_id(),
            Arc::new(Source {
                frames,
                reads: reads.clone(),
                opens: opens.clone(),
                fail,
            }),
            reads,
            opens,
        )
    }
    #[test]
    fn decode_blocks_are_shared_bounded_and_zero_pad_the_tail() {
        let (id, source, reads, _) = source(CHUNK * (BLOCKS + 2) + 7, false);
        let mut cache = Decoded::default();
        let mut out = [[0.; 2]; 40];
        cache.read(id, &source, CHUNK - 20, &mut out).unwrap();
        assert_eq!(out[0], [(CHUNK - 20) as f32; 2]);
        assert_eq!(out[39], [(CHUNK + 19) as f32; 2]);
        cache.frame(id, &source, CHUNK).unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 2);
        for i in 0..BLOCKS + 2 {
            cache.frame(id, &source, i * CHUNK).unwrap();
        }
        assert_eq!(cache.blocks.len(), BLOCKS);
        assert_eq!(cache.order.len(), BLOCKS);
        cache
            .read(id, &source, CHUNK * (BLOCKS + 2), &mut out)
            .unwrap();
        assert_eq!(out[6], [(CHUNK * (BLOCKS + 2) + 6) as f32; 2]);
        assert_eq!(out[7..], [[0.; 2]; 33]);
        for _ in 0..READERS + 2 {
            let (id, source, _, _) = self::source(8, false);
            cache.frame(id, &source, 0).unwrap();
        }
        assert_eq!(cache.readers.len(), READERS);
    }
    #[test]
    fn transient_decode_failure_reopens_without_caching_partial_data() {
        let (id, source, reads, opens) = source(128, true);
        let mut cache = Decoded::default();
        assert_eq!(
            cache.frame(id, &source, 91),
            Err(DecodeFailure::Unavailable)
        );
        assert!(cache.blocks.is_empty() && cache.readers.is_empty());
        assert_eq!(cache.frame(id, &source, 91), Ok([91.; 2]));
        assert_eq!(reads.load(Ordering::Relaxed), 2);
        assert_eq!(opens.load(Ordering::Relaxed), 2);
    }
}
