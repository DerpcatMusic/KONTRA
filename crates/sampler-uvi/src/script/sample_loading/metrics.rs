//! Fixed-size off-audio task timing. Histograms retain every task after any
//! bounded detailed trace fills; wall clocks include scheduling contention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct StageTiming {
    pub count: u64,
    pub total_ns: u64,
    pub max_ns: u64,
    /// Upper bounds in microseconds are 4^i for i=0..14; bucket15 overflows.
    pub histogram: [u64; 16],
}
impl StageTiming {
    pub(super) fn add(&mut self, ns: u64) {
        self.count = self.count.saturating_add(1);
        self.total_ns = self.total_ns.saturating_add(ns);
        self.max_ns = self.max_ns.max(ns);
        let us = ns.div_ceil(1000);
        let bucket = (0..15).find(|i| us <= 1u64 << (i * 2)).unwrap_or(15);
        self.histogram[bucket] = self.histogram[bucket].saturating_add(1);
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct SampleLoadTimings {
    pub metadata_queue: StageTiming,
    pub resolve_and_headers: StageTiming,
    pub path_lookup: StageTiming,
    pub reader_open: StageTiming,
    pub file_metadata: StageTiming,
    pub prepare_decode: StageTiming,
    pub worker_delivery: StageTiming,
    pub ready_submit: StageTiming,
    pub commit_ack: StageTiming,
    pub total: StageTiming,
    pub path_candidates: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleSourceTimings {
    pub path_candidates: u32,
    pub lookup_ns: u64,
    pub reader_open_ns: u64,
    pub metadata_ns: u64,
}
#[derive(Clone, Copy, Default)]
pub(super) struct WorkerTimings {
    pub queue_ns: u64,
    pub resolve_ns: u64,
    pub prepare_ns: u64,
    pub source: SampleSourceTimings,
}
pub(super) fn elapsed(start: std::time::Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
}
impl SampleLoadTimings {
    pub(super) fn worker(&mut self, trace: WorkerTimings) {
        self.metadata_queue.add(trace.queue_ns);
        self.resolve_and_headers.add(trace.resolve_ns);
        self.prepare_decode.add(trace.prepare_ns);
        if trace.source.path_candidates > 0 {
            self.path_candidates = self.path_candidates.saturating_add(trace.source.path_candidates.into());
            self.path_lookup.add(trace.source.lookup_ns);
        }
        if trace.source.reader_open_ns > 0 { self.reader_open.add(trace.source.reader_open_ns); }
        if trace.source.metadata_ns > 0 { self.file_metadata.add(trace.source.metadata_ns); }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_histograms_cover_long_tail_and_saturate_without_wrapping() {
        let mut stage = super::StageTiming::default();
        for ns in [0, 1000, 1001, 4000, 4001, u64::MAX] { stage.add(ns); }
        assert_eq!(stage.count, 6);
        assert_eq!(stage.total_ns, u64::MAX);
        assert_eq!(stage.max_ns, u64::MAX);
        assert_eq!(&stage.histogram[..3], &[2,2,1]);
        assert_eq!(stage.histogram[15], 1);
        assert_eq!(stage.histogram.iter().sum::<u64>(), 6);
    }
}
