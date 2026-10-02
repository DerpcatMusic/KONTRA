//! Opt-in native callback capture. Collection is fixed-size; summary work is off-thread.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub const NATIVE_TIMING_LIMIT: usize = 4096;
const WINDOW: Duration = Duration::from_secs(10);
pub type NativeTimingHook = Arc<dyn Fn(NativeTimingReport) + Send + Sync>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(usize)]
pub enum NativeFrameOutcome {
    #[default]
    NoScene,
    Presented,
    Current,
    Skipped,
    SurfaceLost,
    Error,
    Hidden,
    NoWindow,
    NoGpu,
    Panicked,
}
pub const NATIVE_OUTCOMES: [&str; 10] = [
    "no_scene",
    "present_submitted",
    "current",
    "skipped",
    "surface_lost",
    "error",
    "hidden",
    "no_window",
    "no_gpu",
    "panicked",
];
pub const NATIVE_METRICS: [&str; 7] = [
    "callback_interval_ns",
    "callback_total_ns",
    "shared_lock_wait_ns",
    "advance_ns",
    "scene_clone_ns",
    "resize_ns",
    "present_call_ns",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeFrameSample {
    pub offset_ns: u64,
    pub interval_ns: u64,
    pub total_ns: u64,
    pub lock_ns: u64,
    pub advance_ns: u64,
    pub scene_ns: u64,
    pub resize_ns: u64,
    pub present_ns: u64,
    pub outcome: NativeFrameOutcome,
    pub new_scene: bool,
    pub dragging: bool,
}
impl NativeFrameSample {
    fn values(self) -> [u64; 7] {
        [
            self.interval_ns,
            self.total_ns,
            self.lock_ns,
            self.advance_ns,
            self.scene_ns,
            self.resize_ns,
            self.present_ns,
        ]
    }
}
pub struct NativeTimingReport {
    pub samples: Box<[NativeFrameSample]>,
    pub count: usize,
    pub elapsed_ns: u64,
    pub stop: &'static str,
    pub pointer_moves: u64,
    pub drag_moves: u64,
    pub reentrant_callbacks: u64,
    pub physical_size: (u32, u32),
    pub device_scale: f64,
    pub geometry_changes: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTimingMetric {
    pub count: usize,
    pub mean_ns: f64,
    pub p50_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}
#[derive(Debug)]
pub struct NativeTimingSummary {
    pub metrics: [NativeTimingMetric; 7],
    pub outcomes: [u64; 10],
    pub new_scenes: u64,
    pub dragging_callbacks: u64,
}
impl NativeTimingReport {
    /// Called by the consumer's worker, never from a native callback.
    pub fn summary(&self) -> NativeTimingSummary {
        let samples = &self.samples[..self.count.min(self.samples.len())];
        let metrics = std::array::from_fn(|metric| {
            let mut values: Vec<_> = samples
                .iter()
                .map(|s| s.values()[metric])
                .filter(|v| *v != 0)
                .collect();
            values.sort_unstable();
            let n = values.len();
            if n == 0 {
                return NativeTimingMetric::default();
            }
            NativeTimingMetric {
                count: n,
                mean_ns: values.iter().map(|v| *v as f64).sum::<f64>() / n as f64,
                p50_ns: values[(n - 1) / 2],
                p99_ns: values[((n - 1) * 99).div_ceil(100)],
                max_ns: values[n - 1],
            }
        });
        let mut outcomes = [0; 10];
        for sample in samples {
            outcomes[sample.outcome as usize] += 1;
        }
        NativeTimingSummary {
            metrics,
            outcomes,
            new_scenes: samples.iter().filter(|s| s.new_scene).count() as u64,
            dragging_callbacks: samples.iter().filter(|s| s.dragging).count() as u64,
        }
    }
}

pub(crate) fn ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
pub(crate) fn elapsed(at: Option<Instant>) -> u64 {
    at.map_or(0, |at| ns(at.elapsed()))
}

pub(crate) struct Capture {
    hook: NativeTimingHook,
    samples: Option<Box<[NativeFrameSample]>>,
    count: usize,
    started: Option<Instant>,
    previous: Option<Instant>,
    pub primary: bool,
    pointer_moves: u64,
    drag_moves: u64,
    reentrant: u64,
    size: (u32, u32),
    scale: f64,
    geometry_changes: u64,
}
impl Capture {
    pub fn new(hook: NativeTimingHook, size: (u32, u32), scale: f64) -> Self {
        Self {
            hook,
            samples: Some(
                vec![NativeFrameSample::default(); NATIVE_TIMING_LIMIT].into_boxed_slice(),
            ),
            count: 0,
            started: None,
            previous: None,
            primary: false,
            pointer_moves: 0,
            drag_moves: 0,
            reentrant: 0,
            size,
            scale,
            geometry_changes: 0,
        }
    }
    pub fn geometry(&mut self, size: (u32, u32), scale: f64) {
        if self.active() && (self.size != size || self.scale != scale) {
            self.geometry_changes += 1;
        }
        self.size = size;
        self.scale = scale;
    }
    pub fn active(&self) -> bool {
        self.started.is_some() && self.samples.is_some()
    }
    /// A press alone does not arm: the next actual primary-button pointer move does.
    pub fn pointer_move(&mut self, now: Instant) {
        if self.samples.is_none() {
            return;
        }
        if self.primary && self.started.is_none() {
            self.started = Some(now);
        }
        if self.active() {
            self.pointer_moves += 1;
            self.drag_moves += u64::from(self.primary);
        }
    }
    pub fn begin(&mut self, now: Instant) -> Option<NativeFrameSample> {
        if !self.active() {
            return None;
        }
        let sample = NativeFrameSample {
            offset_ns: ns(now.saturating_duration_since(self.started.unwrap())),
            interval_ns: self
                .previous
                .map_or(0, |at| ns(now.saturating_duration_since(at))),
            dragging: self.primary,
            ..Default::default()
        };
        self.previous = Some(now);
        Some(sample)
    }
    pub fn record(&mut self, sample: NativeFrameSample, now: Instant, reentrant: u64) {
        let Some(samples) = &mut self.samples else {
            return;
        };
        samples[self.count] = sample;
        self.count += 1;
        self.reentrant += reentrant;
        if sample.outcome == NativeFrameOutcome::Panicked {
            self.finish(now, "panic");
        } else if self.count == NATIVE_TIMING_LIMIT {
            self.finish(now, "capacity");
        } else if now.saturating_duration_since(self.started.unwrap()) >= WINDOW {
            self.finish(now, "duration");
        }
    }
    fn finish(&mut self, now: Instant, stop: &'static str) {
        let Some(started) = self.started else {
            return;
        };
        let Some(samples) = self.samples.take() else {
            return;
        };
        (self.hook)(NativeTimingReport {
            samples,
            count: self.count,
            elapsed_ns: ns(now.saturating_duration_since(started)),
            stop,
            pointer_moves: self.pointer_moves,
            drag_moves: self.drag_moves,
            reentrant_callbacks: self.reentrant,
            physical_size: self.size,
            device_scale: self.scale,
            geometry_changes: self.geometry_changes,
        });
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.finish(Instant::now(), "window_closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn bounded_native_capture_summary_and_new_window_reset() {
        let reports = Arc::new(Mutex::new(Vec::new()));
        let output = reports.clone();
        let hook: NativeTimingHook = Arc::new(move |report| output.lock().unwrap().push(report));
        let now = Instant::now();
        let mut capture = Capture::new(hook.clone(), (900, 600), 1.5);
        capture.pointer_move(now);
        assert!(capture.begin(now).is_none(), "hover cannot arm");
        capture.primary = true;
        assert!(capture.begin(now).is_none(), "press alone cannot arm");
        capture.pointer_move(now);
        for i in 0..NATIVE_TIMING_LIMIT {
            let at = now + Duration::from_micros(i as u64);
            let mut sample = capture.begin(at).unwrap();
            sample.total_ns = 20;
            sample.outcome = NativeFrameOutcome::Presented;
            sample.new_scene = true;
            capture.record(sample, at, u64::from(i == 3));
        }
        assert!(
            capture.begin(now).is_none(),
            "one bounded capture per window"
        );
        drop(capture);
        {
            let reports = reports.lock().unwrap();
            assert_eq!(reports.len(), 1);
            let report = &reports[0];
            assert_eq!(
                (report.count, report.samples.len(), report.stop),
                (4096, 4096, "capacity")
            );
            let summary = report.summary();
            assert_eq!(
                summary.outcomes[NativeFrameOutcome::Presented as usize],
                4096
            );
            assert_eq!(summary.metrics[0].count, 4095);
            assert_eq!(summary.metrics[1].mean_ns, 20.);
            assert_eq!(summary.new_scenes, 4096);
            assert_eq!(report.reentrant_callbacks, 1);
        }
        let mut reset = Capture::new(hook.clone(), (800, 500), 1.);
        reset.primary = true;
        reset.pointer_move(now);
        let sample = reset.begin(now).unwrap();
        reset.record(sample, now + WINDOW, 0);
        assert_eq!(reports.lock().unwrap()[1].stop, "duration");
        let mut early = Capture::new(hook, (800, 500), 1.);
        early.primary = true;
        early.pointer_move(now);
        drop(early);
        assert_eq!(reports.lock().unwrap()[2].stop, "window_closed");
    }
}
