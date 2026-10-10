//! Display envelopes computed off audio from the already admitted sample source.
use crate::support::MutexExt;
use sampler_core::{AssetId, Pcm, PlanId};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
};

type Peaks = Arc<[(f32, f32)]>;
type Request = (AssetId, usize, Option<(u64, u64)>);
#[derive(Clone)]
pub(crate) struct Source {
    pub pcm: Pcm,
    pub stream: Option<Arc<dyn sampler_kontakt::AssetSource>>,
}
#[derive(Clone)]
pub(crate) struct Envelope {
    pub peaks: Peaks,
    pub duration_us: u64,
    pub frames: u64,
    pub sample_rate: u32,
    pub range: (u64, u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Source,
    Range,
    Open,
    Header,
    Decode,
    Generation,
    Worker,
}
#[derive(Clone)]
pub(crate) enum State {
    Loading,
    Ready(Envelope),
    Unavailable(Stage),
}
/// Waveform request IDs: physical source IDs where present, else IR ordinals.
pub(crate) fn source_ids(inst: &sampler_ir::Instrument) -> Vec<u32> {
    let mut ids: Vec<u32> = (1..=inst.zones.len() as u32).collect();
    for (physical, zone) in inst.source_indices.zones.iter().enumerate() {
        if let Some(zone) = zone
            && let Some(id) = ids.get_mut(zone.0)
        {
            *id = physical as u32 + 1;
        }
    }
    ids
}
#[derive(Default)]
struct Cache {
    values: HashMap<Request, Result<Envelope, Stage>>,
    pending: HashSet<Request>,
    order: VecDeque<Request>,
}

pub(crate) struct Provider {
    pub plan: PlanId,
    source_ids: HashMap<u32, AssetId>,
    requests: SyncSender<Request>,
    cache: Arc<Mutex<Cache>>,
    stop: Arc<AtomicBool>,
}
impl Provider {
    pub fn start(
        plan: PlanId,
        sources: HashMap<u32, Source>,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let source_ids = sources
            .iter()
            .map(|(&zone, source)| (zone, source.pcm.asset_id()))
            .collect();
        let sources: HashMap<_, _> = sources
            .into_values()
            .map(|source| (source.pcm.asset_id(), source))
            .collect();
        let (requests, incoming) = sync_channel::<Request>(8);
        let cache = Arc::new(Mutex::new(Cache::default()));
        let stop = Arc::new(AtomicBool::new(false));
        std::thread::Builder::new()
            .name("sampler-waveform".into())
            .spawn({
                let cache = cache.clone();
                let stop = stop.clone();
                move || {
                    while let Ok(key) = incoming.recv() {
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let value = sources
                            .get(&key.0)
                            .ok_or(Stage::Source)
                            .and_then(|source| envelope(source, key.1, key.2, &stop));
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        let mut cache = cache.lock_unpoisoned();
                        cache.pending.remove(&key);
                        // At most 64 envelopes (2 MiB); aliases share immutable asset identity.
                        while cache.values.len() >= 64 {
                            if let Some(old) = cache.order.pop_front() {
                                cache.values.remove(&old);
                            }
                        }
                        cache.values.insert(key, value);
                        cache.order.push_back(key);
                        drop(cache);
                        wake();
                    }
                }
            })?;
        Ok(Self {
            plan,
            source_ids,
            requests,
            cache,
            stop,
        })
    }
    pub fn get(&self, zone: u32, bins: usize) -> Option<Envelope> {
        self.get_window(zone, bins, None)
    }
    pub fn get_window(
        &self,
        zone: u32,
        bins: usize,
        range: Option<(u64, u64)>,
    ) -> Option<Envelope> {
        match self.state(zone, bins, range) {
            State::Ready(envelope) => Some(envelope),
            _ => None,
        }
    }
    pub fn state(&self, zone: u32, bins: usize, range: Option<(u64, u64)>) -> State {
        if self.stop.load(Ordering::Acquire) {
            return State::Unavailable(Stage::Worker);
        }
        if zone == 0 || range.is_some_and(|(a, b)| a >= b) {
            return State::Unavailable(if zone == 0 {
                Stage::Source
            } else {
                Stage::Range
            });
        }
        let Some(&asset) = self.source_ids.get(&zone) else {
            return State::Unavailable(Stage::Source);
        };
        let key = (asset, bins.clamp(1, 4096), range);
        let mut cache = self.cache.lock_unpoisoned();
        if let Some(value) = cache.values.get(&key).cloned() {
            cache.order.retain(|old| *old != key);
            cache.order.push_back(key);
            return match value {
                Ok(envelope) => State::Ready(envelope),
                Err(stage) => State::Unavailable(stage),
            };
        }
        if !cache.pending.contains(&key) {
            match self.requests.try_send(key) {
                Ok(()) => {
                    cache.pending.insert(key);
                }
                Err(TrySendError::Disconnected(_)) => return State::Unavailable(Stage::Worker),
                Err(TrySendError::Full(_)) => {}
            }
        }
        State::Loading
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
fn envelope(
    source: &Source,
    bins: usize,
    range: Option<(u64, u64)>,
    stop: &AtomicBool,
) -> Result<Envelope, Stage> {
    if stop.load(Ordering::Acquire) {
        return Err(Stage::Worker);
    }
    let count = source.pcm.frame_count();
    let rate = source.pcm.sample_rate();
    if count == 0 || rate == 0 {
        return Err(Stage::Header);
    }
    let (first, last) = range.unwrap_or((0, count as u64));
    if first >= last || last > count as u64 {
        return Err(Stage::Range);
    }
    let first = usize::try_from(first).map_err(|_| Stage::Range)?;
    let length = usize::try_from(last).map_err(|_| Stage::Range)? - first;
    let resident = source.pcm.resident_frames();
    let mut reader = if resident.is_none() {
        Some(
            source
                .stream
                .as_ref()
                .ok_or(Stage::Open)?
                .open()
                .map_err(|_| Stage::Open)?,
        )
    } else {
        None
    };
    if reader
        .as_ref()
        .is_some_and(|r| r.frames() != count || r.rate() != rate)
    {
        return Err(Stage::Header);
    }
    let bins = bins.clamp(1, 4096);
    let mut peaks = Vec::with_capacity(bins);
    let mut scratch = vec![[0.; 2]; 4096];
    let (mut loaded_start, mut loaded_end) = (first, first);
    for bin in 0..bins {
        let start = first + (bin as u128 * length as u128 / bins as u128) as usize;
        let end = (first + ((bin + 1) as u128 * length as u128 / bins as u128) as usize)
            .max(start + 1)
            .min(first + length);
        let (mut low, mut high) = (1f32, -1f32);
        let mut at = start;
        while at < end {
            if stop.load(Ordering::Acquire) {
                return Err(Stage::Worker);
            }
            let frames = if let Some(frames) = resident {
                &frames[at..end.min(at + scratch.len())]
            } else {
                // v1 wave::read batches the sample, rather than decoding an NCW block per column.
                if at >= loaded_end {
                    let len = (first + length - at).min(scratch.len());
                    reader
                        .as_mut()
                        .ok_or(Stage::Open)?
                        .read(at, &mut scratch[..len])
                        .map_err(|_| Stage::Decode)?;
                    loaded_start = at;
                    loaded_end = at + len;
                }
                &scratch[at - loaded_start..end.min(loaded_end) - loaded_start]
            };
            for sample in frames.iter().flatten() {
                let sample = if sample.is_finite() {
                    sample.clamp(-1., 1.)
                } else {
                    0.
                };
                low = low.min(sample);
                high = high.max(sample);
            }
            at += frames.len();
        }
        peaks.push((low, high));
    }
    Ok(Envelope {
        peaks: peaks.into(),
        range: (first as u64, last),
        frames: count as u64,
        sample_rate: rate,
        duration_us: (count as u128 * 1_000_000 / u128::from(rate)).min(u128::from(u64::MAX))
            as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_ncw_stream_bins_match_resident_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wave.ncw");
        let pcm: Vec<i32> = (0..73200 * 2).map(|i| (i * 37 % 40000) - 20000).collect();
        let bytes = ncw::encode_pcm(
            &pcm,
            ncw::PcmSpec {
                channels: 2,
                bits_per_sample: 16,
                sample_rate: 48000,
            },
            ncw::StereoMode::Direct,
        )
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let decoded = sampler_kontakt::decode(&bytes).unwrap();
        let resident = Source {
            pcm: Pcm::new(decoded.rate, decoded.frames.into_boxed_slice()).unwrap(),
            stream: None,
        };
        let stream = sampler_kontakt::Samples::new(dir.path())
            .source(&path)
            .unwrap();
        let streamed = Source {
            pcm: Pcm::streamed(48000, 73200).unwrap(),
            stream: Some(Arc::new(stream)),
        };
        let stop = AtomicBool::new(false);
        for (bins, range) in [
            (512, None),
            (4096, Some((4095, 8193))),
            (4096, Some((8976, 9020))),
        ] {
            let a = envelope(&resident, bins, range, &stop).unwrap();
            let b = envelope(&streamed, bins, range, &stop).unwrap();
            assert_eq!(a.peaks, b.peaks);
            assert_eq!(a.range, b.range);
            assert_eq!(a.frames, b.frames);
        }
    }

    #[test]
    #[ignore = "owned Conflux NCW stage witness; RAM only, run after quiet"]
    fn screenshot_ncw_source_decode_and_worker_peaks_stage_witness() {
        use sampler_kontakt::AssetSource;
        let root = std::path::Path::new(
            "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]",
        );
        assert!(root.exists(), "owned screenshot library is required");
        let mut samples = sampler_kontakt::Samples::new(root);
        for name in [
            "AI002_WmTSylRlsFrDyn2RR1_1_Fah_50.ncw",
            "AI002_WmTSylAtkClsDyn1RR1_1_Fah_50.ncw",
        ] {
            let location = samples
                .resolve(&root.join("Instruments"), name)
                .expect("source resolution")
                .expect("source member");
            let source = samples.source(&location).expect("source descriptor");
            let mut reader = source.open().expect("native NCW open");
            let count = reader.frames();
            let rate = reader.rate();
            assert!(count > 0 && rate > 0, "nonzero source header");
            if name.contains("RlsFrDyn2") {
                assert_eq!((count, rate), (73200, 48000), "screenshot header identity");
            }
            let mut raw = vec![[0.; 2]; count];
            for (i, chunk) in raw.chunks_mut(4096).enumerate() {
                reader.read(i * 4096, chunk).expect("native NCW decode");
            }
            let nonzero = raw.iter().flatten().filter(|v| v.abs() > 0.00001).count();
            let expected = Source {
                pcm: Pcm::new(rate, raw.into_boxed_slice()).unwrap(),
                stream: None,
            };
            let actual = Source {
                pcm: Pcm::streamed(rate, count).unwrap(),
                stream: Some(Arc::new(source)),
            };
            let stop = AtomicBool::new(false);
            let a = envelope(&expected, 512, None, &stop).expect("resident reference peaks");
            let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 1).unwrap();
            let limits = sampler_core::Limits::for_plan(&prepared, 1, 1);
            let runtime = sampler_core::Runtime::new(prepared, limits).unwrap();
            let (wake, ready) = std::sync::mpsc::channel();
            let provider =
                Provider::start(runtime.active_plan(), [(3, actual)].into(), move || {
                    wake.send(()).ok();
                })
                .unwrap();
            assert!(matches!(provider.state(3, 512, None), State::Loading));
            ready
                .recv_timeout(std::time::Duration::from_secs(15))
                .expect("worker publication");
            let State::Ready(b) = provider.state(3, 512, None) else {
                panic!("worker peaks unavailable");
            };
            assert!(
                a.peaks == b.peaks,
                "worker peaks equal actual native NCW decode without exporting samples"
            );
            let max = b
                .peaks
                .iter()
                .flat_map(|(a, b)| [a.abs(), b.abs()])
                .fold(0f32, f32::max);
            let varied = b.peaks.iter().filter(|(a, b)| b - a > 0.00001).count();
            eprintln!(
                "NCW_STAGE frames={count} rate={rate} nonzero={nonzero} bins={} max={max:.9} varied={varied} resolve=1 decode=1 publication=1",
                b.peaks.len()
            );
            assert!(
                nonzero > 0 && max > 0.00001 && varied > 0,
                "a nonzero screenshot source must publish visible-amplitude peaks"
            );
        }
    }

    #[test]
    fn worker_errors_are_terminal_and_idle_cache_work_is_bounded() {
        let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 1).unwrap();
        let limits = sampler_core::Limits::for_plan(&prepared, 1, 1);
        let runtime = sampler_core::Runtime::new(prepared, limits).unwrap();
        let (wake, ready) = std::sync::mpsc::channel();
        let source = Source {
            pcm: Pcm::streamed(48000, 73200).unwrap(),
            stream: None,
        };
        let provider = Provider::start(runtime.active_plan(), [(3, source)].into(), move || {
            wake.send(()).ok();
        })
        .unwrap();
        assert!(matches!(
            provider.state(0, 512, None),
            State::Unavailable(Stage::Source)
        ));
        assert!(matches!(
            provider.state(9, 512, None),
            State::Unavailable(Stage::Source)
        ));
        assert!(matches!(
            provider.state(3, 512, Some((3, 3))),
            State::Unavailable(Stage::Range)
        ));
        for bins in 1..=65 {
            assert!(matches!(provider.state(3, bins, None), State::Loading));
            ready
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            assert!(matches!(
                provider.state(3, bins, None),
                State::Unavailable(Stage::Open)
            ));
        }
        for _ in 0..1000 {
            assert!(matches!(
                provider.state(3, 65, None),
                State::Unavailable(Stage::Open)
            ));
        }
        let cache = provider.cache.lock_unpoisoned();
        assert_eq!(cache.values.len(), 64);
        assert_eq!(cache.order.len(), 64);
        assert!(cache.pending.is_empty());
        assert!(
            ready.try_recv().is_err(),
            "idle/failure reads must not restart decode or wake the UI"
        );
    }

    #[test]
    fn source_open_header_decode_and_range_failures_keep_their_stage() {
        struct Broken(u8);
        impl sampler_kontakt::AssetSource for Broken {
            fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                if self.0 == 0 {
                    return Err(std::io::ErrorKind::NotFound.into());
                }
                Ok(sampler_kontakt::SampleReader::custom(
                    48000,
                    if self.0 == 1 { 7 } else { 8 },
                    |_, _| Err(std::io::ErrorKind::InvalidData.into()),
                ))
            }
        }
        let stop = AtomicBool::new(false);
        for (code, stage) in [(0, Stage::Open), (1, Stage::Header), (2, Stage::Decode)] {
            let source = Source {
                pcm: Pcm::streamed(48000, 8).unwrap(),
                stream: Some(Arc::new(Broken(code))),
            };
            assert!(matches!(envelope(&source, 4, None, &stop), Err(actual) if actual == stage));
        }
        let source = Source {
            pcm: Pcm::new(48000, vec![[0.; 2]; 8].into_boxed_slice()).unwrap(),
            stream: None,
        };
        assert!(matches!(
            envelope(&source, 4, Some((0, 9)), &stop),
            Err(Stage::Range)
        ));
    }

    #[test]
    fn aliased_zones_reuse_one_asset_envelope() {
        let pcm = Pcm::new(8, vec![[-0.8, 0.6]; 8].into_boxed_slice()).unwrap();
        let source = Source { pcm, stream: None };
        let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 1).unwrap();
        let limits = sampler_core::Limits::for_plan(&prepared, 1, 1);
        let runtime = sampler_core::Runtime::new(prepared, limits).unwrap();
        let (wake, ready) = std::sync::mpsc::channel();
        let provider = Provider::start(
            runtime.active_plan(),
            [(3, source.clone()), (9, source)].into(),
            move || {
                wake.send(()).ok();
            },
        )
        .unwrap();
        assert!(provider.get(3, 4).is_none());
        ready
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let a = provider.get(3, 4).unwrap();
        let b = provider.get(9, 4);
        assert!(
            b.is_some(),
            "another zone sharing the same admitted asset must not queue another decode"
        );
        assert!(Arc::ptr_eq(&a.peaks, &b.unwrap().peaks));
    }

    #[test]
    fn streamed_peak_reads_are_chunked_not_per_bin() {
        use std::sync::atomic::AtomicUsize;
        struct Counted(Arc<AtomicUsize>);
        impl sampler_kontakt::AssetSource for Counted {
            fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                let reads = self.0.clone();
                Ok(sampler_kontakt::SampleReader::custom(
                    48000,
                    73200,
                    move |_, out| {
                        reads.fetch_add(1, Ordering::Relaxed);
                        out.fill([-0.5, 0.25]);
                        Ok(())
                    },
                ))
            }
        }
        let reads = Arc::new(AtomicUsize::new(0));
        let source = Source {
            pcm: Pcm::streamed(48000, 73200).unwrap(),
            stream: Some(Arc::new(Counted(reads.clone()))),
        };
        let e = envelope(&source, 512, None, &AtomicBool::new(false)).unwrap();
        assert!(e.peaks.iter().all(|p| *p == (-0.5, 0.25)));
        assert_eq!(
            reads.load(Ordering::Relaxed),
            73200usize.div_ceil(4096),
            "a peak column must not reopen/decode the same NCW block"
        );
    }
    #[test]
    fn mapping_window_cache_retains_source_identity_and_reuses_peaks() {
        let sources = [
            (
                1,
                Source {
                    pcm: Pcm::new(8, vec![[-0.8, -0.4]; 8].into_boxed_slice()).unwrap(),
                    stream: None,
                },
            ),
            (
                3,
                Source {
                    pcm: Pcm::new(8, vec![[0.2, 0.6]; 8].into_boxed_slice()).unwrap(),
                    stream: None,
                },
            ),
        ]
        .into_iter()
        .collect();
        let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 1).unwrap();
        let limits = sampler_core::Limits::for_plan(&prepared, 1, 1);
        let runtime = sampler_core::Runtime::new(prepared, limits).unwrap();
        let provider = Provider::start(runtime.active_plan(), sources, || {}).unwrap();
        let get = |zone, range| {
            (0..200)
                .find_map(|_| {
                    let e = provider.get_window(zone, 4, Some(range));
                    if e.is_none() {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    e
                })
                .unwrap()
        };
        let a = get(1, (2, 6));
        let b = get(3, (2, 6));
        assert_eq!(a.range, (2, 6));
        assert!(a.peaks.iter().all(|p| *p == (-0.8, -0.4)));
        assert!(b.peaks.iter().all(|p| *p == (0.2, 0.6)));
        assert!(Arc::ptr_eq(&a.peaks, &get(1, (2, 6)).peaks));
        assert!(!Arc::ptr_eq(&a.peaks, &get(1, (3, 6)).peaks));
        assert!(provider.get_window(1, 4, Some((4, 4))).is_none());
    }
    #[test]
    fn mapping_source_ids_preserve_native_holes_and_falcon_ordinals() {
        let mut inst = sampler_ir::Instrument::default();
        for _ in 0..2 {
            inst.zones
                .push(sampler_ir::Zone::new(sampler_ir::AssetRef(0)));
        }
        assert_eq!(source_ids(&inst), vec![1, 2]);
        inst.source_indices.zones = vec![
            Some(sampler_ir::ZoneRef(1)),
            None,
            Some(sampler_ir::ZoneRef(0)),
        ];
        assert_eq!(source_ids(&inst), vec![3, 1]);
    }
    #[test]
    fn resident_and_streamed_envelopes_share_exact_bins_without_head_fallback() {
        let frames = vec![[-2., 0.25], [0.5, 0.], [0.75, -0.75], [0., 2.]];
        let pcm = Pcm::new(4, frames.clone().into_boxed_slice()).unwrap();
        let resident = Source {
            pcm: pcm.clone(),
            stream: None,
        };
        let data = frames.clone();
        struct Stream(Vec<[f32; 2]>);
        impl sampler_kontakt::AssetSource for Stream {
            fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                let frames = self.0.clone();
                let len = frames.len();
                Ok(sampler_kontakt::SampleReader::custom(
                    4,
                    len,
                    move |start, out| {
                        out.copy_from_slice(&frames[start..start + out.len()]);
                        Ok(())
                    },
                ))
            }
        }
        let streamed = Source {
            pcm: Pcm::streamed(4, 4).unwrap(),
            stream: Some(Arc::new(Stream(data))),
        };
        let stop = AtomicBool::new(false);
        let a = envelope(&resident, 2, None, &stop).expect("resident envelope");
        let b = envelope(&streamed, 2, None, &stop).expect("streamed envelope");
        assert_eq!(&*a.peaks, &[(-1., 0.5), (-0.75, 1.)]);
        assert_eq!(&*a.peaks, &*b.peaks);
        assert_eq!(a.duration_us, 1_000_000);
        assert_eq!((a.frames, a.sample_rate), (4, 4));
        assert_eq!((b.frames, b.sample_rate), (4, 4));
        assert!(
            envelope(
                &Source {
                    pcm: streamed.pcm.clone(),
                    stream: None
                },
                2,
                None,
                &stop
            )
            .is_err(),
            "a stream without its reader is unavailable, not a fake full envelope"
        );
        let a = envelope(&resident, 2, Some((1, 3)), &stop).unwrap();
        let b = envelope(&streamed, 2, Some((1, 3)), &stop).unwrap();
        assert_eq!(&*a.peaks, &[(0., 0.5), (-0.75, 0.75)]);
        assert_eq!(&*a.peaks, &*b.peaks);
        assert_eq!(a.range, (1, 3));
        assert_eq!(a.frames, 4);
        assert!(envelope(&resident, 2, Some((2, 2)), &stop).is_err());
        assert!(envelope(&resident, 2, Some((0, 5)), &stop).is_err());
        stop.store(true, Ordering::Release);
        assert!(envelope(&resident, 2, None, &stop).is_err());
    }
}
