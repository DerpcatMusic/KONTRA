//! Display-only UVI preparation. Resolution, metadata and envelope reads stay
//! on this bounded worker; no sample transaction or oscillator setter is used.
use super::{Envelope, Source, envelope};
use sampler_core::Pcm;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

pub(crate) const LIMIT: usize = 64;
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) struct Key {
    pub path: String,
    pub directory: Option<String>,
    pub bins: usize,
}
#[derive(Clone)]
pub(crate) struct Ready {
    pub envelope: Envelope,
    // Keep the exact immutable admitted source alive with its cached envelope.
    // A filename, catalog ordinal or unrelated PCM never supplies this identity.
    pub source: Arc<Source>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Source,
    Envelope,
}
pub(crate) struct Result {
    pub epoch: u64,
    pub key: Key,
    pub value: std::result::Result<Ready, Failure>,
}
struct Batch {
    epoch: u64,
    keys: Vec<Key>,
    cancel: Arc<AtomicBool>,
}
#[derive(Default)]
pub(crate) struct Counts {
    pub resolutions: AtomicU64,
    pub envelopes: AtomicU64,
    pub source_hits: AtomicU64,
    pub completions: AtomicU64,
}
pub(crate) struct Worker {
    jobs: SyncSender<Batch>,
    results: Receiver<Result>,
    pub counts: Arc<Counts>,
}
impl Worker {
    pub fn start(owner: PathBuf, wake: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let (jobs, incoming) = mpsc::sync_channel::<Batch>(1);
        let (done, results) = mpsc::sync_channel(LIMIT);
        let counts = Arc::new(Counts::default());
        let observed = counts.clone();
        std::thread::Builder::new()
            .name("uvi-waveview".into())
            .spawn(move || {
                // The same admitted UVI namespace as other owned resources. Neither
                // bank discovery nor loose filesystem access occurs in an editor frame.
                let mut resources = None;
                let mut sources: VecDeque<((String, Option<String>), Arc<Source>)> =
                    VecDeque::new();
                while let Ok(batch) = incoming.recv() {
                    if batch.cancel.load(Ordering::Acquire) {
                        // Consuming this cancelled job frees queue capacity. A
                        // current display request may have failed try_send while
                        // it occupied the slot; request another preparation frame.
                        wake();
                        continue;
                    }
                    for key in batch.keys {
                        if batch.cancel.load(Ordering::Acquire) {
                            break;
                        }
                        let source_key = (key.path.clone(), key.directory.clone());
                        let source = if let Some(at) =
                            sources.iter().position(|(k, _)| *k == source_key)
                        {
                            observed.source_hits.fetch_add(1, Ordering::Relaxed);
                            let entry = sources.remove(at).unwrap();
                            let source = entry.1.clone();
                            sources.push_back(entry);
                            Some(source)
                        } else {
                            observed.resolutions.fetch_add(1, Ordering::Relaxed);
                            let resolved = resources
                                .get_or_insert_with(|| sampler_uvi::Resources::of(&owner))
                                .sample_result(sampler_uvi::script::SamplePath {
                                    path: &key.path,
                                    script_directory: key.directory.as_deref(),
                                })
                                .ok()
                                .and_then(|mut resource| {
                                    let reader = resource
                                        .reader
                                        .take()
                                        .or_else(|| resource.source.open().ok())?;
                                    let pcm = Pcm::streamed(reader.rate(), reader.frames()).ok()?;
                                    Some(Arc::new(Source {
                                        pcm,
                                        stream: Some(resource.source),
                                    }))
                                });
                            if let Some(source) = &resolved {
                                while sources.len() >= 16 {
                                    sources.pop_front();
                                }
                                sources.push_back((source_key, source.clone()));
                            }
                            resolved
                        };
                        let value = source.ok_or(Failure::Source).and_then(|source| {
                            if batch.cancel.load(Ordering::Acquire) {
                                return Err(Failure::Envelope);
                            }
                            observed.envelopes.fetch_add(1, Ordering::Relaxed);
                            envelope(&source, key.bins, None, &batch.cancel)
                                .map(|envelope| Ready { envelope, source })
                                .ok_or(Failure::Envelope)
                        });
                        if batch.cancel.load(Ordering::Acquire) {
                            break;
                        }
                        if done
                            .send(Result {
                                epoch: batch.epoch,
                                key,
                                value,
                            })
                            .is_err()
                        {
                            return;
                        }
                        observed.completions.fetch_add(1, Ordering::Release);
                        wake();
                    }
                }
            })?;
        Ok(Self {
            jobs,
            results,
            counts,
        })
    }
    pub fn submit(&self, epoch: u64, keys: Vec<Key>, cancel: Arc<AtomicBool>) -> bool {
        keys.len() <= LIMIT
            && self
                .jobs
                .try_send(Batch {
                    epoch,
                    keys,
                    cancel,
                })
                .is_ok()
    }
    pub fn poll(&self) -> Option<Result> {
        self.results.try_recv().ok()
    }
}
