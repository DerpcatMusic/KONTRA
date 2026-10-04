//! Bounded control/UI loading evidence. Never constructed on the audio thread.
use super::{Stats, Status};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct LoadStage {
    pub phase: &'static str,
    pub elapsed: Duration,
    pub outcome: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct ResourceActivity {
    /// Distinct initial resource paths; authored Lua may load additional audio.
    pub total: Option<usize>,
    pub loaded: usize,
    pub unique_decodes: usize,
    /// Resident decoded PCM, shared aliases counted once.
    pub bytes: usize,
    pub current: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorkerLoadActivity {
    pub status: Status,
    pub phase: &'static str,
    pub frame: u64,
    pub elapsed: Duration,
    pub stages: Vec<LoadStage>,
    pub nodes: Option<usize>,
    pub static_rejected_nodes: Option<usize>,
    pub sample_zones: Option<usize>,
    pub script_processors: Option<usize>,
    pub resources: ResourceActivity,
    pub failure: Option<String>,
    pub stats: Stats,
}
