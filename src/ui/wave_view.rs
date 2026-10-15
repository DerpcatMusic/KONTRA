//! Assets-owned request fence around the display-only waveform worker.
use crate::sound::waveform::uvi_manual::{self, Failure, Key, Ready, Worker};
use sampler_ui_ir as ir;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) struct Preparation {
    owner: PathBuf,
    program: Option<u32>,
    worker: Option<Worker>,
    started: bool,
    epoch: u64,
    cancel: Arc<AtomicBool>,
    wanted: HashMap<ir::WidgetRef, Key>,
    pending: HashSet<Key>,
    // Requests that could not enter the bounded worker queue on this frame.
    // They still keep preparation alive until a later frame can submit them.
    deferred: usize,
    cache: VecDeque<(Key, Result<Ready, Failure>)>,
    pub revision: u64,
    pub requested: u64,
    pub stale: u64,
    pub dropped: u64,
}

#[cfg(test)]
pub(super) fn authored_wav(amplitude: i16) -> Vec<u8> {
    let samples: Vec<_> = (0..4096)
        .flat_map(|n| (if n % 2 == 0 { amplitude } else { -amplitude }).to_le_bytes())
        .collect();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + samples.len() as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(8000u32.to_le_bytes());
    bytes.extend(16000u32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((samples.len() as u32).to_le_bytes());
    bytes.extend(samples);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    fn face(sample: ir::UviWaveViewSample) -> ir::Interface {
        let mut widget = ir::Widget::new(
            "manual",
            ir::PageRef(0),
            ir::Rect::new(0, 0, 240, 96),
            ir::Kind::Waveform,
        );
        widget.uvi_wave_view_request = Some(ir::UviWaveViewRequest {
            sample,
            ..Default::default()
        });
        ir::Interface {
            source: ir::Source::FalconLua,
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 320.,
                    height: 200.,
                },
                ..Default::default()
            }],
            widgets: vec![widget],
            ..Default::default()
        }
    }
    fn wait(p: &mut Preparation, face: &ir::Interface) -> Ready {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            p.prepare(face, ir::PageRef(0), 1.);
            if let Some(ready) = p.get(
                ir::WidgetRef(0),
                face.widgets[0].uvi_wave_view_request.as_ref().unwrap(),
            ) {
                return ready.clone();
            }
            assert!(
                std::time::Instant::now() < end,
                "real display source was not serviced"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    #[test]
    fn manual_wave_view_real_provider_a_b_clear_a_reuses_exact_source_and_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let owner = dir.path().join("authored.uvip");
        std::fs::write(&owner, "<UVI4><Program/></UVI4>").unwrap();
        std::fs::write(dir.path().join("a.wav"), authored_wav(4096)).unwrap();
        std::fs::write(dir.path().join("b.wav"), authored_wav(24576)).unwrap();
        let mut p = Preparation::new(&owner, Some(0));
        let a_face = face(ir::UviWaveViewSample::Manual("a.wav".into()));
        let a = wait(&mut p, &a_face);
        assert!(a.envelope.peaks.iter().all(|&p| p == (-0.125, 0.125)));
        let b = wait(&mut p, &face(ir::UviWaveViewSample::Manual("b.wav".into())));
        assert!(b.envelope.peaks.iter().all(|&p| p == (-0.75, 0.75)));
        p.prepare(&face(ir::UviWaveViewSample::Cleared), ir::PageRef(0), 1.);
        assert!(p.wanted.is_empty());
        assert_eq!(p.pending(), 0);
        let again = wait(&mut p, &a_face);
        assert!(Arc::ptr_eq(&a.envelope.peaks, &again.envelope.peaks));
        assert!(Arc::ptr_eq(&a.source, &again.source));
        assert_eq!(p.counts(), (2, 2, 2));
        assert!(p.bytes() <= 64 * 4096 * 8);
    }
    #[test]
    fn manual_wave_view_queue_backpressure_keeps_current_source_pending_until_submitted() {
        let dir = tempfile::tempdir().unwrap();
        let owner = dir.path().join("authored.uvip");
        std::fs::write(&owner, "<UVI4><Program/></UVI4>").unwrap();
        for (name, amplitude) in [("a.wav", 4096), ("b.wav", 8192), ("c.wav", 16384)] {
            std::fs::write(dir.path().join(name), authored_wav(amplitude)).unwrap();
        }
        // Hold the real provider after completing A, before it can consume B.
        // C then encounters a full queue containing the now-cancelled B epoch.
        let (completed, provider_wake) = std::sync::mpsc::channel();
        let (resume, held) = std::sync::mpsc::sync_channel(1);
        let first = AtomicBool::new(true);
        let mut p = Preparation::new(&owner, None);
        p.worker = Some(
            Worker::start(owner, move || {
                let _ = completed.send(());
                if first.swap(false, Ordering::AcqRel) {
                    let _ = held.recv();
                }
            })
            .unwrap(),
        );
        p.started = true;
        let a = face(ir::UviWaveViewSample::Manual("a.wav".into()));
        p.prepare(&a, ir::PageRef(0), 1.);
        provider_wake
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        p.prepare(&a, ir::PageRef(0), 1.);
        assert!(
            p.get(
                ir::WidgetRef(0),
                a.widgets[0].uvi_wave_view_request.as_ref().unwrap()
            )
            .is_some()
        );
        p.prepare(
            &face(ir::UviWaveViewSample::Manual("b.wav".into())),
            ir::PageRef(0),
            1.,
        );
        let c = face(ir::UviWaveViewSample::Manual("c.wav".into()));
        p.prepare(&c, ir::PageRef(0), 1.);
        let pending_before_resume = p.pending();
        let submitted_before_resume = p.counts().0;
        resume.send(()).unwrap();
        assert_eq!(
            submitted_before_resume, 2,
            "C has not entered the full queue"
        );
        assert_eq!(
            pending_before_resume, 1,
            "preparation must keep waiting for C"
        );
        // A cancelled queued job must request another preparation frame;
        // no successful source result for C exists yet to wake the UI instead.
        provider_wake
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("cancelled B must wake the deferred C preparation");
        let current = wait(&mut p, &c);
        assert!(current.envelope.peaks.iter().all(|&p| p == (-0.5, 0.5)));
        assert_eq!(p.pending(), 0);
        assert_eq!(p.counts(), (3, 2, 2), "cancelled B never resolves a source");
    }
    #[test]
    fn manual_wave_view_discards_completed_stale_epoch_and_retries_failures_only_after_change() {
        let dir = tempfile::tempdir().unwrap();
        let owner = dir.path().join("authored.uvip");
        std::fs::write(&owner, "<UVI4><Program/></UVI4>").unwrap();
        std::fs::write(dir.path().join("a.wav"), authored_wav(4096)).unwrap();
        let mut p = Preparation::new(&owner, None);
        let a = face(ir::UviWaveViewSample::Manual("a.wav".into()));
        p.prepare(&a, ir::PageRef(0), 1.);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while p
            .worker
            .as_ref()
            .unwrap()
            .counts
            .completions
            .load(Ordering::Acquire)
            == 0
        {
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        p.prepare(&face(ir::UviWaveViewSample::Cleared), ir::PageRef(0), 1.);
        assert_eq!(p.stale, 1);
        assert!(p.cache.is_empty());
        let _ = wait(&mut p, &a);
        assert_eq!(
            p.counts(),
            (2, 1, 2),
            "old completion cannot clear a new same-key request"
        );
        let missing = face(ir::UviWaveViewSample::Manual("missing.wav".into()));
        p.prepare(&missing, ir::PageRef(0), 1.);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while p.pending() != 0 {
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(1));
            p.prepare(&missing, ir::PageRef(0), 1.);
        }
        let failed = p.counts();
        std::fs::write(dir.path().join("missing.wav"), authored_wav(8192)).unwrap();
        p.prepare(&missing, ir::PageRef(0), 1.);
        assert_eq!(
            p.counts(),
            failed,
            "a frame does not retry the same failed epoch"
        );
        p.prepare(&face(ir::UviWaveViewSample::Cleared), ir::PageRef(0), 1.);
        let _ = wait(&mut p, &missing);
        assert_eq!(p.counts().1, failed.1 + 1);
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl Preparation {
    pub fn new(owner: &Path, program: Option<u32>) -> Self {
        Self {
            owner: owner.into(),
            program,
            worker: None,
            started: false,
            epoch: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            wanted: HashMap::new(),
            pending: HashSet::new(),
            deferred: 0,
            cache: VecDeque::new(),
            revision: 0,
            requested: 0,
            stale: 0,
            dropped: 0,
        }
    }
    pub fn same_owner(&self, owner: &Path, program: Option<u32>) -> bool {
        self.owner == owner && self.program == program
    }
    pub fn prepare(&mut self, face: &ir::Interface, page: ir::PageRef, scale: f64) {
        let mut wanted = HashMap::new();
        for (n, widget) in face.widgets.iter().enumerate() {
            let n = ir::WidgetRef(n);
            if widget.page != page || !face.visible(n) {
                continue;
            }
            let Some(request) = &widget.uvi_wave_view_request else {
                continue;
            };
            let ir::UviWaveViewSample::Manual(path) = &request.sample else {
                continue;
            };
            if path.is_empty()
                || path.len() > 4096
                || path.contains('\0')
                || request
                    .script_directory
                    .as_ref()
                    .is_some_and(|p| p.len() > 4096 || p.contains('\0'))
            {
                continue;
            }
            if wanted.len() >= uvi_manual::LIMIT {
                self.dropped += 1;
                continue;
            }
            wanted.insert(
                n,
                Key {
                    path: path.clone(),
                    directory: request.script_directory.clone(),
                    bins: (face.page_rect(n).width * scale).ceil().clamp(1., 4096.) as usize,
                },
            );
        }
        // Fence before polling: a completion for A can never enter B/clear's
        // visible state, including a same-key request in a subsequent epoch.
        if wanted != self.wanted {
            self.cancel.store(true, Ordering::Release);
            self.cancel = Arc::new(AtomicBool::new(false));
            self.epoch = self.epoch.wrapping_add(1);
            self.wanted = wanted;
            self.pending.clear();
            self.deferred = 0;
            if self.worker.is_none() {
                self.started = false;
            }
            self.cache.retain(|(_, value)| value.is_ok()); // a new request epoch may retry failure
            self.revision = self.revision.wrapping_add(1);
        }
        if !self.wanted.is_empty() && !self.started {
            self.started = true;
            self.worker =
                Worker::start(self.owner.clone(), super::super::picture_worker::completed).ok();
        }
        let Some(worker) = &self.worker else {
            return;
        };
        while let Some(result) = worker.poll() {
            if result.epoch != self.epoch || !self.wanted.values().any(|key| *key == result.key) {
                self.stale += 1;
                continue;
            }
            self.pending.remove(&result.key);
            self.cache.retain(|(key, _)| *key != result.key);
            while self.cache.len() >= uvi_manual::LIMIT {
                self.cache.pop_front();
            }
            self.cache.push_back((result.key, result.value));
            self.revision = self.revision.wrapping_add(1);
        }
        let missing: HashSet<_> = self
            .wanted
            .values()
            .filter(|key| {
                !self.pending.contains(*key) && !self.cache.iter().any(|(old, _)| old == *key)
            })
            .cloned()
            .collect();
        self.deferred = missing.len();
        if !missing.is_empty()
            && worker.submit(
                self.epoch,
                missing.iter().cloned().collect(),
                self.cancel.clone(),
            )
        {
            self.requested += missing.len() as u64;
            self.pending.extend(missing);
            self.deferred = 0;
        }
    }
    pub fn get(&self, widget: ir::WidgetRef, request: &ir::UviWaveViewRequest) -> Option<&Ready> {
        let key = self.wanted.get(&widget)?;
        if !matches!(&request.sample, ir::UviWaveViewSample::Manual(path) if *path == key.path)
            || request.script_directory != key.directory
        {
            return None;
        }
        self.cache
            .iter()
            .find(|(old, _)| old == key)?
            .1
            .as_ref()
            .ok()
    }
    #[cfg(feature = "shots")]
    pub fn status(&self, widget: ir::WidgetRef, request: &ir::UviWaveViewRequest) -> &'static str {
        match &request.sample {
            ir::UviWaveViewSample::Unspecified => "unset",
            ir::UviWaveViewSample::Cleared => "cleared",
            ir::UviWaveViewSample::Unsupported { .. } => "type_invalid",
            ir::UviWaveViewSample::Manual(path)
                if path.is_empty() || path.len() > 4096 || path.contains('\0') =>
            {
                "path_invalid"
            }
            ir::UviWaveViewSample::Manual(_) if self.get(widget, request).is_some() => "ready",
            ir::UviWaveViewSample::Manual(_) if !self.wanted.contains_key(&widget) => {
                "not_requested"
            }
            ir::UviWaveViewSample::Manual(_) => {
                if self.wanted.get(&widget).is_some_and(|key| {
                    self.cache
                        .iter()
                        .any(|(old, result)| old == key && result.is_err())
                }) {
                    "source_unavailable"
                } else if self.started && self.worker.is_none() {
                    "worker_unavailable"
                } else {
                    "pending"
                }
            }
        }
    }
    pub fn pending(&self) -> usize {
        self.pending.len() + self.deferred
    }
    pub fn bytes(&self) -> usize {
        self.cache
            .iter()
            .filter_map(|(_, value)| value.as_ref().ok())
            .map(|ready| ready.envelope.peaks.len() * std::mem::size_of::<(f32, f32)>())
            .sum()
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.wanted
            .values()
            .filter_map(|key| {
                self.cache
                    .iter()
                    .find(|(old, _)| old == key)?
                    .1
                    .as_ref()
                    .err()
                    .map(|failure| format!("WaveView manual display: {failure:?}"))
            })
            .collect()
    }
    #[cfg(any(test, feature = "shots"))]
    pub fn counts(&self) -> (u64, u64, u64) {
        self.worker.as_ref().map_or((0, 0, 0), |worker| {
            (
                self.requested,
                worker.counts.resolutions.load(Ordering::Relaxed),
                worker.counts.envelopes.load(Ordering::Relaxed),
            )
        })
    }
}
