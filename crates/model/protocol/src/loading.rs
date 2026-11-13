//! Lock-free observations of the instrument currently loading.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// The user-visible phase of an instrument load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum LoadStage {
    #[default]
    Reading,
    Samples,
    Scripts,
    Interface,
}

impl LoadStage {
    /// A short description for the editor.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Reading => "Reading instrument",
            Self::Samples => "Loading samples",
            Self::Scripts => "Preparing scripts",
            Self::Interface => "Building interface",
        }
    }
}

/// A snapshot of the current load, read without a lock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadSnapshot {
    pub stage: LoadStage,
    pub percent: u8,
    pub bytes: u64,
}

// Rounded mean shares of the five measured instrument loads. The split is an
// estimate; sample progress uses the actual pinned bytes for this load.
const READING_END: u8 = 67;
const SAMPLES_END: u8 = 85;
const SCRIPTS_END: u8 = 91;

/// Shared loader counters, never touched by the audio callback.
#[derive(Debug, Default)]
pub struct LoadProgress {
    stage: AtomicU8,
    total: AtomicU64,
    bytes: AtomicU64,
}

impl LoadProgress {
    /// Starts a new instrument load.
    pub fn begin(&self) {
        self.total.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
        self.stage.store(0, Ordering::Release);
    }

    /// Advances to the next phase.
    pub fn stage(&self, stage: LoadStage) {
        self.stage.store(stage as u8, Ordering::Release);
    }

    /// Sets the expected bytes for the sample phase.
    pub fn sample_total(&self, bytes: u64) {
        self.total.store(bytes, Ordering::Relaxed);
    }

    /// Adds sample bytes that have been loaded or reused.
    pub fn samples(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Reads the visible phase, fraction, and sample bytes.
    #[must_use]
    pub fn snapshot(&self) -> LoadSnapshot {
        let stage = match self.stage.load(Ordering::Acquire) {
            1 => LoadStage::Samples,
            2 => LoadStage::Scripts,
            3 => LoadStage::Interface,
            _ => LoadStage::Reading,
        };
        let bytes = self.bytes.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        let percent = match stage {
            LoadStage::Reading => 0,
            LoadStage::Samples => {
                let span = SAMPLES_END - READING_END;
                READING_END
                    + if total == 0 {
                        0
                    } else {
                        (bytes.saturating_mul(u64::from(span)) / total).min(u64::from(span)) as u8
                    }
            }
            LoadStage::Scripts => SAMPLES_END,
            LoadStage::Interface => SCRIPTS_END,
        };
        LoadSnapshot {
            stage,
            percent,
            bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LoadProgress, LoadStage};

    #[test]
    fn a_load_advances_from_reading_to_interface_and_reports_sample_bytes() {
        let progress = LoadProgress::default();
        progress.begin();
        let mut last = progress.snapshot();
        assert_eq!(last.stage, LoadStage::Reading);
        progress.stage(LoadStage::Samples);
        progress.sample_total(1_000_000);
        for expected in [200_000, 400_000, 1_000_000] {
            progress.samples(expected - last.bytes);
            let next = progress.snapshot();
            assert!(next.percent >= last.percent);
            assert_eq!(next.bytes, expected);
            last = next;
        }
        for stage in [LoadStage::Scripts, LoadStage::Interface] {
            progress.stage(stage);
            let next = progress.snapshot();
            assert!(next.percent >= last.percent);
            assert_eq!(next.bytes, 1_000_000);
            last = next;
        }
    }
}
