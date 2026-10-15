//! Bounded startup attack preparation. No initial PCM is published until every
//! source has succeeded and exact packed capacity has been admitted.
use super::{AssetId, AssetSource, Pcm, Range, SampleReader, canceled_head, prepare_head_ranges};
use sampler_core::{HeadBudget, PreparedRanges, ResidentBuffer};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Instant,
};

pub(super) enum Prepared {
    Head(PreparedRanges),
    Resident(ResidentBuffer),
}
impl Prepared {
    fn bytes(&self) -> usize {
        match self {
            Self::Head(h) => h.bytes(),
            Self::Resident(f) => f.bytes(),
        }
    }
}
pub(super) fn fill_resident(
    rate: u32,
    frames: usize,
    mut buffer: ResidentBuffer,
    reader: &mut SampleReader,
    canceled: &dyn Fn() -> bool,
    mut timing: Option<&mut Timing>,
) -> io::Result<ResidentBuffer> {
    if reader.rate() != rate || reader.frames() != frames {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "resident source header changed",
        ));
    }
    for (index, page) in buffer
        .frames_mut()
        .chunks_mut(sampler_core::PAGE_FRAMES)
        .enumerate()
    {
        if canceled() {
            return Err(canceled_head());
        }
        let start = timing.as_ref().map(|_| Instant::now());
        reader.read(index * sampler_core::PAGE_FRAMES, page)?;
        if let Some(t) = timing.as_mut() {
            t.read_decode_us += start.unwrap().elapsed().as_micros() as u64;
        }
        if canceled() {
            return Err(canceled_head());
        }
        let start = timing.as_ref().map(|_| Instant::now());
        if page.iter().flatten().any(|v| !v.is_finite()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "non-finite resident source",
            ));
        }
        if let Some(t) = timing.as_mut() {
            t.validate_pack_us += start.unwrap().elapsed().as_micros() as u64;
        }
    }
    Ok(buffer)
}

#[derive(Default)]
pub(super) struct Timing {
    pub read_decode_us: u64,
    pub validate_pack_us: u64,
}
#[derive(Default)]
struct Distribution {
    count: u64,
    total_us: u64,
    max_us: u64,
    log2_us: [u64; 32],
}
impl Distribution {
    fn add(&mut self, us: u64) {
        self.count += 1;
        self.total_us = self.total_us.saturating_add(us);
        self.max_us = self.max_us.max(us);
        self.log2_us[(63 - us.max(1).leading_zeros() as usize).min(31)] += 1;
    }
    fn json(&self) -> serde_json::Value {
        serde_json::json!({"count":self.count,"total_us":self.total_us,"max_us":self.max_us,"log2_us":self.log2_us})
    }
}
struct Audit {
    worker: usize,
    jobs: u64,
    failures: u64,
    wait: Distribution,
    open: Distribution,
    read: Distribution,
    pack: Distribution,
    total: Distribution,
    probes: Vec<serde_json::Value>,
    dropped: u64,
}
impl Audit {
    fn new(worker: usize) -> Self {
        Self {
            worker,
            jobs: 0,
            failures: 0,
            wait: Default::default(),
            open: Default::default(),
            read: Default::default(),
            pack: Default::default(),
            total: Default::default(),
            probes: Vec::new(),
            dropped: 0,
        }
    }
    fn record(
        &mut self,
        index: usize,
        asset: AssetId,
        wait: u64,
        open: u64,
        timing: &Timing,
        total: u64,
        result: &io::Result<Prepared>,
    ) {
        self.jobs += 1;
        self.failures += u64::from(result.is_err());
        self.wait.add(wait);
        self.open.add(open);
        self.read.add(timing.read_decode_us);
        self.pack.add(timing.validate_pack_us);
        self.total.add(total);
        if self.probes.len() < 64 {
            self.probes.push(serde_json::json!({"index":index,"asset":format!("{asset:?}"),"startup_batch_wait_us":wait,"open_us":open,"seek_read_decode_us":timing.read_decode_us,"validate_pack_us":timing.validate_pack_us,"worker_total_us":total,"payload_bytes":result.as_ref().map(|r|r.bytes()).ok(),"payload_kind":result.as_ref().ok().map(|r|match r {Prepared::Head(_)=>"packed_head",Prepared::Resident(_)=>"full_resident"}),"packed_bytes":result.as_ref().ok().and_then(|r|match r {Prepared::Head(h)=>Some(h.bytes()),Prepared::Resident(_)=>None}),"success":result.is_ok(),"error_kind":result.as_ref().err().map(|e|format!("{:?}",e.kind()))}));
        } else {
            self.dropped += 1;
        }
    }
}
impl Drop for Audit {
    fn drop(&mut self) {
        eprintln!(
            "head_prepare_audit {}",
            serde_json::json!({"worker":self.worker,"jobs":self.jobs,"failures":self.failures,"startup_batch_wait":self.wait.json(),"open":self.open.json(),"seek_read_decode":self.read.json(),"validate_pack":self.pack.json(),"worker_total":self.total.json(),"probes":self.probes,"dropped_probes":self.dropped,"scope":"startup queue; stage totals sum worker time and are not wall time; read/decode are combined"})
        );
    }
}

#[allow(
    deprecated,
    reason = "fetch_update preserves the declared Rust 1.92 minimum"
)]
pub(super) fn prepare(
    selected: &[(&Pcm, &[Range<usize>])],
    sources: &HashMap<AssetId, Arc<dyn AssetSource>>,
    budget: &HeadBudget,
    requested_workers: usize,
    canceled: &(dyn Fn() -> bool + Sync),
) -> io::Result<(Vec<Prepared>, usize)> {
    let _wall = crate::audit::Span::new("sample_head_prepare");
    if selected.is_empty() {
        return if canceled() {
            Err(canceled_head())
        } else {
            Ok((Vec::new(), budget.status().held_bytes))
        };
    }
    // Mandatory full buffers reserve first, before any ordinary packed work.
    // These exact reservations cannot lose a race to optional attack storage.
    let resident = selected
        .iter()
        .map(|(pcm, _)| {
            if canceled() {
                return Err(canceled_head());
            }
            let full = if pcm.requires_resident() {
                Some(
                    ResidentBuffer::new(pcm.sample_rate(), pcm.frame_count(), budget)
                        .map_err(io::Error::other)?,
                )
            } else {
                None
            };
            Ok(Mutex::new(full))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let workers = requested_workers.max(1).min(8).min(selected.len());
    let audit_enabled = std::env::var_os("KONTRA_AUDIT_LOAD").is_some();
    let batch_start = audit_enabled.then(Instant::now);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let error = Mutex::new(None::<io::Error>);
    let fail = |failure: io::Error| {
        let mut first = error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if first.is_none() {
            *first = Some(failure);
        }
        stop.store(true, Ordering::Release);
    };
    let mut prepared: Vec<Option<Prepared>> = (0..selected.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let (send, receive) = mpsc::sync_channel(workers);
        let mut handles = Vec::with_capacity(workers);
        for worker in 0..workers {
            let send = send.clone();
            let (next, stop, fail, resident) = (&next, &stop, &fail, &resident);
            let handle = std::thread::Builder::new()
                .name("sample-head-prepare".into())
                .spawn_scoped(scope, move || {
                    let mut audit = audit_enabled.then(|| Audit::new(worker));
                    let mut page = vec![[0.; 2]; sampler_core::PAGE_FRAMES];
                    loop {
                        if stop.load(Ordering::Acquire) || canceled() {
                            break;
                        }
                        let Ok(index) =
                            next.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                                (n < selected.len()).then(|| n + 1)
                            })
                        else {
                            break;
                        };
                        let (pcm, ranges) = selected[index];
                        let started = audit_enabled.then(Instant::now);
                        let wait = started.zip(batch_start).map_or(0, |(now, start)| {
                            now.duration_since(start).as_micros() as u64
                        });
                        let mut timing = Timing::default();
                        let mut open_us = 0;
                        let result = (|| {
                            let source = sources.get(&pcm.asset_id()).ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::NotFound,
                                    "missing sample head source",
                                )
                            })?;
                            let open_start = audit_enabled.then(Instant::now);
                            let opened = source.open();
                            open_us = open_start.map_or(0, |t| t.elapsed().as_micros() as u64);
                            let mut reader: SampleReader = opened?;
                            if pcm.requires_resident() {
                                let buffer = resident[index]
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .take()
                                    .ok_or_else(|| {
                                        io::Error::other("missing mandatory source reservation")
                                    })?;
                                return fill_resident(
                                    pcm.sample_rate(),
                                    pcm.frame_count(),
                                    buffer,
                                    &mut reader,
                                    &|| stop.load(Ordering::Acquire) || canceled(),
                                    audit_enabled.then_some(&mut timing),
                                )
                                .map(Prepared::Resident);
                            }
                            prepare_head_ranges(
                                &pcm.head_preparer(),
                                &mut reader,
                                ranges,
                                &mut page,
                                &|| stop.load(Ordering::Acquire) || canceled(),
                                audit_enabled.then_some(&mut timing),
                            )
                            .map(Prepared::Head)
                        })();
                        if let Some(audit) = &mut audit {
                            audit.record(
                                index,
                                pcm.asset_id(),
                                wait,
                                open_us,
                                &timing,
                                started.unwrap().elapsed().as_micros() as u64,
                                &result,
                            );
                        }
                        match result {
                            Ok(prepared) => {
                                if send.send((index, prepared)).is_err() {
                                    break;
                                }
                            }
                            Err(failure) => {
                                fail(failure);
                                break;
                            }
                        }
                    }
                });
            match handle {
                Ok(handle) => handles.push(handle),
                Err(failure) => {
                    fail(failure);
                    break;
                }
            }
        }
        drop(send);
        for (index, ranges) in receive {
            prepared[index] = Some(ranges);
        }
        for handle in handles {
            if handle.join().is_err() {
                fail(io::Error::other("sample head worker panicked"));
            }
        }
    });
    if canceled() {
        return Err(canceled_head());
    }
    if let Some(error) = error
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return Err(error);
    }
    let ranges = prepared
        .into_iter()
        .map(|p| p.ok_or_else(|| io::Error::other("incomplete sample head preparation")))
        .collect::<io::Result<Vec<_>>>()?;
    Ok((ranges, budget.status().held_bytes))
}
