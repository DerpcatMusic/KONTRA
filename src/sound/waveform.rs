//! Display envelopes computed off audio from the already admitted sample source.
use sampler_core::{Pcm, PlanId};
use std::{collections::{HashMap, HashSet, VecDeque}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc::{SyncSender, sync_channel}}};

type Peaks = Arc<[(f32, f32)]>;
type Request = (u32, usize, Option<(u64, u64)>);
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
/// Waveform request IDs: physical source IDs where present, else IR ordinals.
pub(crate) fn source_ids(inst: &sampler_ir::Instrument) -> Vec<u32> {
    let mut ids: Vec<u32> = (1..=inst.zones.len() as u32).collect();
    for (physical, zone) in inst.source_indices.zones.iter().enumerate() {
        if let Some(zone) = zone && let Some(id) = ids.get_mut(zone.0) { *id = physical as u32 + 1; }
    }
    ids
}
#[derive(Default)]
struct Cache {
    values: HashMap<Request, Option<Envelope>>,
    pending: HashSet<Request>,
    order: VecDeque<Request>,
}

pub(crate) struct Provider {
    pub plan: PlanId,
    requests: SyncSender<Request>,
    cache: Arc<Mutex<Cache>>,
    stop: Arc<AtomicBool>,
}
impl Provider {
    pub fn start(plan: PlanId, sources: HashMap<u32, Source>, wake: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let (requests, incoming) = sync_channel::<Request>(8);
        let cache = Arc::new(Mutex::new(Cache::default()));
        let stop = Arc::new(AtomicBool::new(false));
        std::thread::Builder::new().name("sampler-waveform".into()).spawn({
            let cache = cache.clone(); let stop = stop.clone();
            move || while let Ok(key) = incoming.recv() {
                if stop.load(Ordering::Acquire) { break; }
                let value = sources.get(&key.0).and_then(|source| envelope(source, key.1, key.2, &stop));
                if stop.load(Ordering::Acquire) { break; }
                let mut cache = cache.lock().unwrap();
                cache.pending.remove(&key);
                // ponytail: at most 64 envelopes (2 MiB); a byte LRU if this grows.
                while cache.values.len() >= 64 {
                    if let Some(old) = cache.order.pop_front() { cache.values.remove(&old); }
                }
                cache.values.insert(key, value); cache.order.push_back(key);
                drop(cache); wake();
            }
        })?;
        Ok(Self {plan, requests, cache, stop})
    }
    pub fn get(&self, zone: u32, bins: usize) -> Option<Envelope> { self.get_window(zone, bins, None) }
    pub fn get_window(&self, zone: u32, bins: usize, range: Option<(u64, u64)>) -> Option<Envelope> {
        if zone == 0 || range.is_some_and(|(a,b)| a >= b) { return None; }
        let key = (zone, bins.clamp(1, 4096), range);
        let mut cache = self.cache.lock().unwrap();
        if let Some(value) = cache.values.get(&key).cloned() {
            cache.order.retain(|old| *old != key); cache.order.push_back(key);
            return value;
        }
        if !cache.pending.contains(&key) && self.requests.try_send(key).is_ok() { cache.pending.insert(key); }
        None
    }
}
impl Drop for Provider {
    fn drop(&mut self) { self.stop.store(true, Ordering::Release); }
}
fn envelope(source: &Source, bins: usize, range: Option<(u64,u64)>, stop: &AtomicBool) -> Option<Envelope> {
    if stop.load(Ordering::Acquire) { return None; }
    let count = source.pcm.frame_count();
    let rate = source.pcm.sample_rate();
    if count == 0 || rate == 0 { return None; }
    let (first, last) = range.unwrap_or((0, count as u64));
    if first >= last || last > count as u64 { return None; }
    let first = usize::try_from(first).ok()?;
    let length = usize::try_from(last).ok()? - first;
    let resident = source.pcm.resident_frames();
    let mut reader = if resident.is_none() { Some(source.stream.as_ref()?.open().ok()?) } else { None };
    if reader.as_ref().is_some_and(|r| r.frames() != count || r.rate() != rate) { return None; }
    let bins = bins.clamp(1, 4096);
    let mut peaks = Vec::with_capacity(bins);
    let mut scratch = vec![[0.; 2]; 4096];
    for bin in 0..bins {
        let start = first + (bin as u128 * length as u128 / bins as u128) as usize;
        let end = (first + ((bin + 1) as u128 * length as u128 / bins as u128) as usize).max(start + 1).min(first + length);
        let (mut low, mut high) = (1f32, -1f32);
        let mut at = start;
        while at < end {
            if stop.load(Ordering::Acquire) { return None; }
            let len = (end - at).min(scratch.len());
            let frames = if let Some(frames) = resident { &frames[at..at + len] } else {
                reader.as_mut()?.read(at, &mut scratch[..len]).ok()?; &scratch[..len]
            };
            for sample in frames.iter().flatten() {
                let sample = if sample.is_finite() { sample.clamp(-1., 1.) } else { 0. };
                low = low.min(sample); high = high.max(sample);
            }
            at += len;
        }
        peaks.push((low, high));
    }
    Some(Envelope {peaks:peaks.into(), range:(first as u64,last), frames:count as u64, sample_rate:rate, duration_us:(count as u128 * 1_000_000 / u128::from(rate)).min(u128::from(u64::MAX)) as u64})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_window_cache_retains_source_identity_and_reuses_peaks() {
        let sources=[(1,Source {pcm:Pcm::new(8,vec![[-0.8,-0.4];8].into_boxed_slice()).unwrap(),stream:None}),
            (3,Source {pcm:Pcm::new(8,vec![[0.2,0.6];8].into_boxed_slice()).unwrap(),stream:None})].into_iter().collect();
        let prepared=sampler_core::Prepared::new(48000,vec![],vec![],1).unwrap();
        let limits=sampler_core::Limits::for_plan(&prepared,1,1);
        let runtime=sampler_core::Runtime::new(prepared,limits).unwrap();
        let provider=Provider::start(runtime.active_plan(),sources,||{}).unwrap();
        let get=|zone, range| (0..200).find_map(|_|{let e=provider.get_window(zone,4,Some(range));if e.is_none(){std::thread::sleep(std::time::Duration::from_millis(2));}e}).unwrap();
        let a=get(1,(2,6));let b=get(3,(2,6));
        assert_eq!(a.range,(2,6));assert!(a.peaks.iter().all(|p|*p==(-0.8,-0.4)));
        assert!(b.peaks.iter().all(|p|*p==(0.2,0.6)));
        assert!(Arc::ptr_eq(&a.peaks,&get(1,(2,6)).peaks));
        assert!(!Arc::ptr_eq(&a.peaks,&get(1,(3,6)).peaks));
        assert!(provider.get_window(1,4,Some((4,4))).is_none());
    }
    #[test]
    fn mapping_source_ids_preserve_native_holes_and_falcon_ordinals() {
        let mut inst=sampler_ir::Instrument::default();
        for _ in 0..2 {inst.zones.push(sampler_ir::Zone::new(sampler_ir::AssetRef(0)));}
        assert_eq!(source_ids(&inst),vec![1,2]);
        inst.source_indices.zones=vec![Some(sampler_ir::ZoneRef(1)),None,Some(sampler_ir::ZoneRef(0))];
        assert_eq!(source_ids(&inst),vec![3,1]);
    }
    #[test]
    fn resident_and_streamed_envelopes_share_exact_bins_without_head_fallback() {
        let frames = vec![[-2., 0.25], [0.5, 0.], [0.75, -0.75], [0., 2.]];
        let pcm = Pcm::new(4, frames.clone().into_boxed_slice()).unwrap();
        let resident = Source {pcm: pcm.clone(), stream: None};
        let data = frames.clone();
        struct Stream(Vec<[f32;2]>);
        impl sampler_kontakt::AssetSource for Stream {
            fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                let frames = self.0.clone(); let len = frames.len();
                Ok(sampler_kontakt::SampleReader::custom(4, len, move |start, out| {out.copy_from_slice(&frames[start..start+out.len()]); Ok(())}))
            }
        }
        let streamed = Source {pcm:Pcm::streamed(4,4).unwrap(), stream:Some(Arc::new(Stream(data)))};
        let stop = AtomicBool::new(false);
        let a = envelope(&resident, 2, None, &stop).expect("resident envelope");
        let b = envelope(&streamed, 2, None, &stop).expect("streamed envelope");
        assert_eq!(&*a.peaks, &[(-1.,0.5),(-0.75,1.)]);
        assert_eq!(&*a.peaks, &*b.peaks); assert_eq!(a.duration_us, 1_000_000);
        assert_eq!((a.frames,a.sample_rate),(4,4)); assert_eq!((b.frames,b.sample_rate),(4,4));
        assert!(envelope(&Source {pcm:streamed.pcm.clone(),stream:None},2,None,&stop).is_none(),"a stream without its reader is unavailable, not a fake full envelope");
        let a=envelope(&resident,2,Some((1,3)),&stop).unwrap();
        let b=envelope(&streamed,2,Some((1,3)),&stop).unwrap();
        assert_eq!(&*a.peaks,&[(0.,0.5),(-0.75,0.75)]);
        assert_eq!(&*a.peaks,&*b.peaks);assert_eq!(a.range,(1,3));assert_eq!(a.frames,4);
        assert!(envelope(&resident,2,Some((2,2)),&stop).is_none());
        assert!(envelope(&resident,2,Some((0,5)),&stop).is_none());
        stop.store(true,Ordering::Release); assert!(envelope(&resident,2,None,&stop).is_none());
    }
}
