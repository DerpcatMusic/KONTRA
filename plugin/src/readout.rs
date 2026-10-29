//! The header's load readout: the audio thread's CPU, the disk's streaming
//! rate, sample and process memory. Taken twice a second from counters, so
//! a figure is a mean over that span (as v2's `src/ui/mod.rs` meters) and
//! an idle rack reads a steady 0.

use std::time::{Duration, Instant};

use kontra_host::rack::Rack;
use kontra_protocol::Stats;

const EVERY: Duration = Duration::from_millis(500);
const MB: f64 = 1_048_576.0;

/// What the header shows, and the counters it was taken from.
#[derive(Debug, Default)]
pub struct Readout {
    at: Option<Instant>,
    busy: (u64, u64),
    read: u64,
    /// Percent of the audio thread's real time spent rendering.
    pub cpu: f64,
    pub disk_mbs: f64,
    pub cache_mb: u32,
    /// The samples' preloaded pages.
    pub samples_mb: u32,
    /// The whole process; `None` where the platform is not read.
    pub process_mb: Option<u32>,
}

impl Readout {
    /// Takes a new look when half a second passed since the last.
    pub fn sample(&mut self, stats: &Stats, rack: &Rack) {
        let now = Instant::now();
        let since = self.at.map(|t| now - t);
        if since.is_some_and(|s| s < EVERY) {
            return;
        }
        self.at = Some(now);
        let busy = (stats.busy_ns, stats.span_ns);
        let (work, span) = (
            busy.0.saturating_sub(self.busy.0),
            busy.1.saturating_sub(self.busy.1),
        );
        self.busy = busy;
        let load = if span > 0 {
            work as f64 / span as f64 * 100.0
        } else {
            0.0
        };
        let stream = rack.store().stats();
        let read = stream.bytes_read.saturating_sub(self.read) as f64 / MB;
        self.read = stream.bytes_read;
        let rate = since.map_or(0.0, |s| read / s.as_secs_f64());
        // Eased, as v2's: one preempted block is scheduling, not work.
        self.cpu = settle(self.cpu * 0.5 + load * 0.5, 0.5);
        self.disk_mbs = settle(self.disk_mbs * 0.5 + rate * 0.5, 0.05);
        self.cache_mb = mb(stream.cache_bytes);
        self.samples_mb = mb(rack.store().pinned_bytes());
        self.process_mb = resident().map(mb);
    }

    /// As the header prints it, for redraw detection.
    pub fn shown(&self) -> (i64, i64, u32, Option<u32>) {
        let cpu = self.cpu.round() as i64;
        let disk = (self.disk_mbs * 10.0).round() as i64;
        (cpu, disk, self.samples_mb, self.process_mb)
    }
}

fn settle(v: f64, floor: f64) -> f64 {
    if v < floor { 0.0 } else { v }
}

fn mb(bytes: u64) -> u32 {
    u32::try_from(bytes >> 20).unwrap_or(u32::MAX)
}

/// The process's resident bytes.
#[cfg(target_os = "linux")]
fn resident() -> Option<u64> {
    // ponytail: 4 KiB pages; a 16 KiB-page kernel reads a quarter.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

#[cfg(not(target_os = "linux"))]
fn resident() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_host::rack::{Store, StoreConfig};

    #[test]
    fn the_readout_is_the_audio_threads_mean_load_over_its_span() {
        let store = Store::new(StoreConfig::DEFAULT).unwrap();
        let (rack, mut audio) = Rack::new(48_000, store, 480);
        for _ in 0..100 {
            audio.process(480, &[]);
        }
        let stats = rack.stats().unwrap();
        assert_eq!(stats.span_ns, 1_000_000_000);
        assert!(stats.busy_ns > 0 && stats.busy_ns < stats.span_ns);
        let mut r = Readout::default();
        let at = |busy_ns, span_ns| Stats {
            busy_ns,
            span_ns,
            ..stats
        };
        r.sample(&at(0, 0), &rack);
        r.at = r.at.map(|t| t - EVERY);
        r.sample(&at(500_000_000, 1_000_000_000), &rack);
        // Eased half way to 50%.
        assert_eq!(r.cpu, 25.0);
        assert_eq!(r.cache_mb, 128);
        if cfg!(target_os = "linux") {
            assert!(r.process_mb.is_some_and(|m| m > 0));
        }
    }
}
