//! Dedicated allocating UVI playback thread with fixed, bounded packet transport.
//! Start, inspect private failures, stop and destroy the controller off the audio
//! thread. Only `Realtime` packet methods belong in an audio callback. This moves
//! work off that callback; it does not establish a CPU deadline or real-time claim.

use super::{
    host::UiSnapshot,
    library::{BankResources, Library},
    player::{self, Player},
    program::NodeId,
    script::{HostCompletion, HostRoot, HostedInput, Input, InputKind},
};
use anyhow::{Context, Result, ensure};
use crossbeam_queue::ArrayQueue;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const BLOCK_FRAMES: usize = 256;
pub const MAX_INPUTS: usize = 256;
pub const QUEUE_CAPACITY: usize = 8;
pub const COMPLETION_CAPACITY: usize = 4096;
pub const MAX_HOSTED_INPUTS: usize = MAX_INPUTS;
pub use player::{MAX_UI_EDITS, UiInput};
const POLL: Duration = Duration::from_millis(1);

/// Epoch changes on every activation, even when the instrument/rate stays the same.
/// Generation identifies the selected instrument. Frames are absolute within an
/// activation, starting at zero; the controller never resets a running Player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub epoch: u64,
    pub generation: u64,
    pub frame: u64,
}

/// Caller-owned local authority; no reader namespaces or keys enter packet queues.
/// Deliberately has no Debug implementation containing private content state.
pub struct StartConfig {
    pub bank: PathBuf,
    /// Catalog identity checked again against the bank actually opened here.
    pub expected_bank_uuid: Option<[u8; 16]>,
    pub member: String,
    pub metadata_namespace: Vec<u8>,
    pub program_namespace: Vec<u8>,
    pub content_key: Option<u64>,
    pub content_bank: Option<String>,
    pub sample_rate: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    WrongEpoch,
    WrongGeneration,
    WrongFrame,
    InvalidInput,
    TooManyInputs,
    Full,
    Underrun,
    Failed,
    Stopped,
    PortTaken,
}

impl std::fmt::Display for PacketError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, formatter)
    }
}
impl std::error::Error for PacketError {}

#[derive(Debug, Clone, Copy)]
pub struct Request {
    pub stamp: Stamp,
    pub input_count: u16,
    pub inputs: [Input; MAX_INPUTS],
    pub ui_count: u16,
    pub ui_inputs: [UiInput; MAX_UI_EDITS],
}

impl Request {
    /// Fixed stack/inline storage only. Validation is repeated at submission, so
    /// changing public packet fields cannot bypass the queue's trust boundary.
    pub fn new(stamp: Stamp, inputs: &[Input]) -> std::result::Result<Self, PacketError> {
        Self::new_with_ui(stamp, inputs, &[])
    }

    pub fn new_with_ui(
        stamp: Stamp,
        inputs: &[Input],
        ui_inputs: &[UiInput],
    ) -> std::result::Result<Self, PacketError> {
        if inputs.len() > MAX_INPUTS || ui_inputs.len() > MAX_UI_EDITS {
            return Err(PacketError::TooManyInputs);
        }
        let mut packet = Self {
            stamp,
            input_count: inputs.len() as u16,
            inputs: [Input {
                frame: 0,
                kind: InputKind::NoteOff {
                    channel: 0,
                    note: 0,
                },
            }; MAX_INPUTS],
            ui_count: ui_inputs.len() as u16,
            ui_inputs: [UiInput {
                frame: 0,
                edit: super::host::UiEdit {
                    processor: 0,
                    widget: 0,
                    value: super::host::UiEditValue::Push,
                    modifiers: super::host::UiModifiers::default(),
                },
            }; MAX_UI_EDITS],
        };
        packet.inputs[..inputs.len()].copy_from_slice(inputs);
        packet.ui_inputs[..ui_inputs.len()].copy_from_slice(ui_inputs);
        packet.validate()?;
        Ok(packet)
    }

    fn validate(&self) -> std::result::Result<(), PacketError> {
        let count = usize::from(self.input_count);
        let ui_count = usize::from(self.ui_count);
        if count > MAX_INPUTS || ui_count > MAX_UI_EDITS {
            return Err(PacketError::TooManyInputs);
        }
        let end = self
            .stamp
            .frame
            .checked_add(BLOCK_FRAMES as u64)
            .ok_or(PacketError::WrongFrame)?;
        if !self.stamp.frame.is_multiple_of(BLOCK_FRAMES as u64) {
            return Err(PacketError::WrongFrame);
        }
        let inputs = &self.inputs[..count];
        if inputs.iter().any(|input| {
            input.frame < self.stamp.frame || input.frame >= end || !player::input_is_valid(input)
        }) || inputs.windows(2).any(|pair| pair[0].frame > pair[1].frame)
        {
            return Err(PacketError::InvalidInput);
        }
        let ui_inputs = &self.ui_inputs[..ui_count];
        if ui_inputs.iter().any(|input| {
            input.frame < self.stamp.frame
                || input.frame >= end
                || !player::ui_input_is_valid(input)
        }) || ui_inputs
            .windows(2)
            .any(|pair| pair[0].frame > pair[1].frame)
        {
            return Err(PacketError::InvalidInput);
        }
        Ok(())
    }
}

/// Opt-in hosted packet. Ordinary MIDI plus hosted entries share MAX_INPUTS;
/// UI has its separate MAX_UI_EDITS bound. Keep controls and rooted notes in one
/// hosted array to preserve their order, including at equal frames. Separate
/// ordinary MIDI remains compatible: UI, hosted, then ordinary at equal frames.
#[derive(Clone, Copy, Debug)]
pub struct HostedRequest {
    pub request: Request,
    pub root_count: u16,
    pub roots: [HostedInput; MAX_HOSTED_INPUTS],
}
impl HostedRequest {
    pub fn new(request: Request, roots: &[HostedInput]) -> std::result::Result<Self, PacketError> {
        if roots.len() > MAX_HOSTED_INPUTS {
            return Err(PacketError::TooManyInputs);
        }
        let mut packet = Self {
            request,
            root_count: roots.len() as u16,
            roots: [HostedInput::Off {
                root: HostRoot {
                    epoch: 0,
                    generation: 0,
                    token: 0,
                },
                frame: 0,
            }; MAX_HOSTED_INPUTS],
        };
        packet.roots[..roots.len()].copy_from_slice(roots);
        packet.validate()?;
        Ok(packet)
    }
    fn validate(&self) -> std::result::Result<(), PacketError> {
        self.request.validate()?;
        let count = usize::from(self.root_count);
        if count > MAX_HOSTED_INPUTS || count + usize::from(self.request.input_count) > MAX_INPUTS {
            return Err(PacketError::TooManyInputs);
        }
        let stamp = self.request.stamp;
        let end = stamp.frame + BLOCK_FRAMES as u64; // Request checked overflow.
        let mut last_frame = stamp.frame;
        let mut last_on = 0;
        for entry in &self.roots[..count] {
            let (root, frame) = match *entry {
                HostedInput::On { root, input } => {
                    if !player::input_is_valid(&input)
                        || !matches!(input.kind, InputKind::NoteOn { .. })
                        || root.token <= last_on
                    {
                        return Err(PacketError::InvalidInput);
                    }
                    last_on = root.token;
                    (Some(root), input.frame)
                }
                HostedInput::Off { root, frame } | HostedInput::Choke { root, frame } => {
                    (Some(root), frame)
                }
                HostedInput::Event(input) => {
                    if !player::input_is_valid(&input)
                        || matches!(
                            input.kind,
                            InputKind::NoteOn { .. } | InputKind::NoteOff { .. }
                        )
                    {
                        return Err(PacketError::InvalidInput);
                    }
                    (None, input.frame)
                }
            };
            if root.is_some_and(|root| {
                root.epoch != stamp.epoch || root.generation != stamp.generation || root.token == 0
            }) || frame < last_frame
                || frame >= end
            {
                return Err(PacketError::InvalidInput);
            }
            last_frame = frame;
        }
        // Session validates authoritative lifetime only after dequeuing. No
        // callback-side root ledger is created, or mutated by a full submission.
        Ok(())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct HostedRejected {
    pub reason: PacketError,
    pub request: HostedRequest,
}

/// A rejected request is returned intact. Full queues do not advance the input
/// cursor: retry this packet, or explicitly abort/replace the activation.
#[derive(Debug, Clone, Copy)]
pub struct Rejected {
    pub reason: PacketError,
    pub request: Request,
}

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub stamp: Stamp,
    pub audio: [[f32; 2]; BLOCK_FRAMES],
    pub commands: u32,
    pub host_commands: u32,
    pub logs: u32,
    pub dropped_logs: u32,
    /// Matches the UI input indices in the corresponding fixed request packet.
    pub rejected_ui: u64,
}

/// Durable root completion, independent of droppable/late PCM packets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StampedCompletion {
    pub stamp: Stamp,
    pub root: HostRoot,
}

/// Control/UI-thread response. The owned snapshot can contain private captions
/// and artwork references, so this packet deliberately has no Debug implementation.
pub struct UiSnapshotReply {
    pub request: u64,
    /// Activation identity and the exclusive playback boundary at capture time.
    pub stamp: Stamp,
    pub processor: NodeId,
    pub snapshot: std::result::Result<UiSnapshot, UiSnapshotError>,
}

/// Requested control-only runtime inspection. No Lua callbacks, asset paths or
/// source payloads are part of this snapshot. Shared ownership keeps report
/// readers from copying the graph while the worker prepares a newer request.
pub struct RuntimeSnapshotReply {
    pub request: u64,
    pub stamp: Stamp,
    pub snapshot: std::result::Result<Arc<serde_json::Value>, RuntimeSnapshotError>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSnapshotError {
    Unavailable,
}

/// Control-only, all processors at one native boundary; private data, no Debug.
pub struct StateSnapshotReply {
    pub request: u64,
    pub stamp: Stamp,
    pub snapshot: std::result::Result<super::state::SavedState, StateSnapshotError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateSnapshotError {
    Execution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSnapshotError {
    Unavailable,
}

#[derive(Clone, Copy)]
struct UiSnapshotRequest {
    id: u64,
    processor: NodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Status {
    Starting,
    Ready,
    Failed,
    Stopped,
}

#[path = "worker_activity.rs"]
mod activity;
pub use activity::{LoadStage, ResourceActivity, WorkerLoadActivity};

#[derive(Debug, Default, Clone, Copy, serde::Serialize)]
pub struct Stats {
    /// Rejected submissions (excluding Full) and fatal worker failures.
    pub errors: u64,
    /// `try_receive` calls with no output available at the requested stamp.
    /// Includes empty or future output; prefetch peeks are not counted.
    /// This is a polling counter, not an audible or audio-driver shortage.
    pub empty_output_polls: u64,
    /// Legacy alias of `empty_output_polls`, retained for API/report compatibility.
    pub underruns: u64,
    pub backpressure: u64,
    pub stale_packets: u64,
    pub rendered_blocks: u64,
    /// Exclusive boundary of the latest successfully rendered packet.
    pub processed_frame: u64,
    /// Lua print records returned by successfully rendered packets.
    pub logs: u64,
    /// Print records omitted by the bounded Lua log sink in those packets.
    pub dropped_logs: u64,
    /// Retained renderer voice instances after its latest completed packet;
    /// includes held/releasing silent voices, not an audibility estimate.
    /// Cleared when the activation stops or fails.
    pub active_voices: u64,
    /// Historical census at processed_frame, retained after stop/failure.
    pub last_completed_voices: u64,
    pub initialization_ns: u64,
    /// Player.render only: excludes initialization and queue waiting.
    pub render_ns: u64,
    pub max_render_ns: u64,
    /// Render calls exceeding 256/sample_rate seconds, not arrival deadlines.
    pub render_deadline_misses: u64,
    pub cancelled_requests: u64,
    pub cancelled_outputs: u64,
    /// Durable shared-queue and controller-held inline records discarded on
    /// cancellation. Detached AudioPort inline records and untransferred
    /// Session EndReady roots are excluded.
    pub cancelled_completions: u64,
}

#[derive(Default)]
struct Counters {
    errors: AtomicU64,
    empty_output_polls: AtomicU64,
    backpressure: AtomicU64,
    stale_packets: AtomicU64,
    rendered_blocks: AtomicU64,
    processed_frame: AtomicU64,
    logs: AtomicU64,
    dropped_logs: AtomicU64,
    active_voices: AtomicU64,
    last_completed_voices: AtomicU64,
    initialization_ns: AtomicU64,
    render_ns: AtomicU64,
    max_render_ns: AtomicU64,
    render_deadline_misses: AtomicU64,
    cancelled_requests: AtomicU64,
    cancelled_outputs: AtomicU64,
    cancelled_completions: AtomicU64,
}

/// A fixed startup sequence, sampled only by loader/control inspection.
/// It excludes UI asset preparation and host adoption, which this worker does
/// not own. Existing LoadTrace retains the corresponding journal timings.
struct InitializationTiming {
    started: Instant,
    current: Option<(&'static str, Instant)>,
    stages: Vec<(&'static str, Duration, &'static str)>,
    finished: Option<(Duration, &'static str)>,
}
impl Default for InitializationTiming {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            current: Some(("starting", now)),
            stages: Vec::with_capacity(16),
            finished: None,
        }
    }
}
impl InitializationTiming {
    fn stage(&mut self, name: &'static str) {
        if self.finished.is_some() {
            return;
        }
        if let Some((previous, started)) = self.current.take() {
            self.stages.push((previous, started.elapsed(), "finished"));
        }
        self.current = Some((name, Instant::now()));
    }
    fn finish(&mut self, outcome: &'static str) {
        if self.finished.is_some() {
            return;
        }
        if let Some((name, started)) = self.current.take() {
            self.stages.push((
                name,
                started.elapsed(),
                if outcome == "ready" {
                    "finished"
                } else {
                    outcome
                },
            ));
        }
        self.finished = Some((self.started.elapsed(), outcome));
    }
    fn report(&self) -> serde_json::Value {
        let mut stages: Vec<_> = self.stages.iter().map(|(name, elapsed, outcome)|
            serde_json::json!({"phase":name,"elapsed_ms":elapsed.as_secs_f64()*1000.,"outcome":outcome})).collect();
        if let Some((name, started)) = self.current {
            stages.push(serde_json::json!({"phase":name,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"outcome":"in_progress"}));
        }
        serde_json::json!({"outcome":self.finished.map_or("in_progress",|(_,outcome)|outcome),
            "elapsed_ms":self.finished.map_or_else(||self.started.elapsed(),|(elapsed,_)|elapsed).as_secs_f64()*1000.,
            "stages":stages,"scope":"worker_initialization_only",
            "ui_assets_measured":false,"host_adoption_wait_measured":false,
            "lua_time_includes_authored_resource_loads":true,
            "renderer_time_includes_internal_preflight_and_sample_validation":false,
            "static_preflight_reused_for_initial_player_and_renderer":true,
            "renderer_time_includes_runtime_rate_sample_and_graph_validation":true})
    }
}

#[derive(Default)]
struct Details {
    initialization: InitializationTiming,
    resource_activity: ResourceActivity,
    activity_cache: Option<(Instant, &'static str, Status, usize, Arc<WorkerLoadActivity>)>,
    failure: Option<String>,
    lua_failure: Option<Arc<super::lua_failure::Context>>,
    phase: &'static str,
    phase_frame: u64,
    program_report: Option<Arc<serde_json::Value>>,
    mapping: Option<Arc<super::mapping::Inspection>>,
    load_report: Option<Arc<serde_json::Value>>,
    diagnostics: Vec<&'static str>,
    ui_processors: Vec<NodeId>,
    initialized_ui: Option<Arc<Vec<UiSnapshot>>>,
    ui_latest: u64,
    ui_request: Option<UiSnapshotRequest>,
    ui_reply: Option<UiSnapshotReply>,
    state_latest: u64,
    state_request: Option<(u64, u64)>,
    state_reply: Option<StateSnapshotReply>,
    runtime_latest: u64,
    runtime_request: Option<(u64, u64)>,
    runtime_reply: Option<RuntimeSnapshotReply>,
    runtime_report: Option<Arc<serde_json::Value>>,
}

struct Shared {
    stamp: Stamp,
    requests: ArrayQueue<Request>,
    outputs: ArrayQueue<Output>,
    hosted: Option<Box<HostedTransport>>,
    stop: AtomicBool,
    status: AtomicU8,
    counters: Counters,
    details: Mutex<Details>,
    ui_pending: AtomicBool,
    state_pending: AtomicBool,
    runtime_pending: AtomicBool,
}

struct HostedTransport {
    requests: ArrayQueue<HostedRequest>,
    completions: ArrayQueue<StampedCompletion>,
}

impl Shared {
    fn new(epoch: u64, generation: u64) -> Self {
        Self::new_mode(epoch, generation, false)
    }
    fn new_mode(epoch: u64, generation: u64, hosted: bool) -> Self {
        Self {
            stamp: Stamp {
                epoch,
                generation,
                frame: 0,
            },
            requests: ArrayQueue::new(QUEUE_CAPACITY),
            outputs: ArrayQueue::new(QUEUE_CAPACITY),
            hosted: hosted.then(|| {
                Box::new(HostedTransport {
                    requests: ArrayQueue::new(QUEUE_CAPACITY),
                    completions: ArrayQueue::new(COMPLETION_CAPACITY),
                })
            }),
            stop: AtomicBool::new(false),
            status: AtomicU8::new(Status::Starting as u8),
            counters: Counters::default(),
            details: Mutex::new(Details {
                phase: "starting",
                ..Details::default()
            }),
            ui_pending: AtomicBool::new(false),
            state_pending: AtomicBool::new(false),
            runtime_pending: AtomicBool::new(false),
        }
    }

    fn status(&self) -> Status {
        match self.status.load(Ordering::Acquire) {
            0 => Status::Starting,
            1 => Status::Ready,
            2 => Status::Failed,
            _ => Status::Stopped,
        }
    }

    fn stats(&self) -> Stats {
        let c = &self.counters;
        let empty_output_polls = c.empty_output_polls.load(Ordering::Relaxed);
        Stats {
            errors: c.errors.load(Ordering::Relaxed),
            empty_output_polls,
            underruns: empty_output_polls,
            backpressure: c.backpressure.load(Ordering::Relaxed),
            stale_packets: c.stale_packets.load(Ordering::Relaxed),
            rendered_blocks: c.rendered_blocks.load(Ordering::Relaxed),
            processed_frame: c.processed_frame.load(Ordering::Relaxed),
            logs: c.logs.load(Ordering::Relaxed),
            dropped_logs: c.dropped_logs.load(Ordering::Relaxed),
            active_voices: c.active_voices.load(Ordering::Relaxed),
            last_completed_voices: c.last_completed_voices.load(Ordering::Relaxed),
            initialization_ns: c.initialization_ns.load(Ordering::Relaxed),
            render_ns: c.render_ns.load(Ordering::Relaxed),
            max_render_ns: c.max_render_ns.load(Ordering::Relaxed),
            render_deadline_misses: c.render_deadline_misses.load(Ordering::Relaxed),
            cancelled_requests: c.cancelled_requests.load(Ordering::Relaxed),
            cancelled_outputs: c.cancelled_outputs.load(Ordering::Relaxed),
            cancelled_completions: c.cancelled_completions.load(Ordering::Relaxed),
        }
    }

    fn activation(&self, stamp: Stamp) -> std::result::Result<(), PacketError> {
        if stamp.epoch != self.stamp.epoch {
            return Err(PacketError::WrongEpoch);
        }
        if stamp.generation != self.stamp.generation {
            return Err(PacketError::WrongGeneration);
        }
        if self.stop.load(Ordering::Acquire) {
            return Err(PacketError::Stopped);
        }
        match self.status() {
            Status::Failed => Err(PacketError::Failed),
            Status::Stopped => Err(PacketError::Stopped),
            _ => Ok(()),
        }
    }
}

/// Off-audio-thread controller. Dropping it stops and joins the worker; never
/// transfer responsibility for this controller's destruction to the audio thread.
pub struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    cursor: Option<PacketCursor>,
}

#[derive(Default)]
struct PacketCursor {
    next_request: u64,
    minimum_output: u64,
    pending_output: Option<Output>,
    completion_boundary: u64,
    pending_completion: Option<StampedCompletion>,
}

/// Exclusive packet endpoint created on the control thread. Its packet methods
/// are fixed and nonblocking; its owned storage must be retired and destroyed on
/// the control thread. Keep the Worker there until this endpoint is detached.
/// It deliberately has neither Clone nor a worker-thread handle.
pub struct AudioPort {
    shared: Arc<Shared>,
    cursor: PacketCursor,
}

impl AudioPort {
    pub fn realtime(&mut self) -> Realtime<'_> {
        Realtime {
            shared: &self.shared,
            cursor: Some(&mut self.cursor),
        }
    }
}

impl Worker {
    /// Configured activation identity; frame remains zero, not a playback clock.
    pub(crate) fn activation_stamp(&self) -> Stamp {
        self.shared.stamp
    }

    pub fn start(config: StartConfig, epoch: u64, generation: u64) -> Result<Self> {
        Self::start_inner(config, epoch, generation, false, None)
    }
    pub fn start_hosted(config: StartConfig, epoch: u64, generation: u64) -> Result<Self> {
        Self::start_inner(config, epoch, generation, true, None)
    }
    /// Off audio. Saved script/widget data never grants filesystem/content authority.
    pub fn start_hosted_with_state(
        config: StartConfig,
        epoch: u64,
        generation: u64,
        saved: super::state::SavedState,
    ) -> Result<Self> {
        Self::start_inner(config, epoch, generation, true, Some(saved))
    }
    fn start_inner(
        config: StartConfig,
        epoch: u64,
        generation: u64,
        hosted: bool,
        saved: Option<super::state::SavedState>,
    ) -> Result<Self> {
        ensure!(
            (8000..=192000).contains(&config.sample_rate),
            "Invalid UVI worker sample rate"
        );
        ensure!(
            !config.metadata_namespace.is_empty(),
            "UVI worker requires a metadata namespace"
        );
        let shared = Arc::new(if hosted {
            Shared::new_mode(epoch, generation, true)
        } else {
            Shared::new(epoch, generation)
        });
        let worker_shared = shared.clone();
        let handle = thread::Builder::new()
            .name("kontra-uvi-playback".into())
            .spawn(move || {
                let initialized = Instant::now();
                let mut trace = crate::diagnostics::LoadTrace::new(&config.bank, 0, None);
                trace.detail("backend", "native_uvi");
                trace.detail("member", config.member.clone());
                trace.detail("epoch", epoch);
                trace.detail("generation", generation);
                trace.detail("sample_rate", config.sample_rate);
                trace.detail("hosted", hosted);
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run(
                        config,
                        &worker_shared,
                        initialized,
                        saved.as_ref(),
                        &mut trace,
                    )
                }));
                let failure = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("{error:#}")),
                    Err(_) => Some("UVI playback worker panicked".to_owned()),
                };
                if worker_shared
                    .counters
                    .initialization_ns
                    .load(Ordering::Relaxed)
                    == 0
                {
                    worker_shared
                        .counters
                        .initialization_ns
                        .store(nanos(initialized.elapsed()), Ordering::Relaxed);
                }
                worker_shared
                    .details
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .initialization
                    .finish(if failure.is_some() {
                        "failed"
                    } else {
                        "cancelled"
                    });
                if let Some(reason) = &failure {
                    let details = worker_shared
                        .details
                        .lock()
                        .unwrap_or_else(|p| p.into_inner());
                    trace.detail("failure_phase", details.phase);
                    trace.detail("failure_frame", details.phase_frame);
                    drop(details);
                    trace.fail(reason.clone());
                }
                let report = trace.finish(if failure.is_some() {
                    "failed"
                } else {
                    "stopped"
                });
                worker_shared
                    .details
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .load_report = Some(report);
                finish(&worker_shared, failure);
            })
            .context("Starting UVI playback worker")?;
        Ok(Self {
            shared,
            thread: Some(handle),
            cursor: Some(PacketCursor::default()),
        })
    }

    /// Control thread only. Extraction is one-time: subsequent controller packet
    /// access returns PortTaken, while status and UI mailboxes remain available.
    pub fn take_audio_port(&mut self) -> Option<AudioPort> {
        let cursor = self.cursor.take()?;
        Some(AudioPort {
            shared: Arc::clone(&self.shared),
            cursor,
        })
    }

    /// Temporary exclusive borrow, suitable for one callback. No allocation,
    /// reference-count change or destructor is attached to this packet port.
    pub fn realtime(&mut self) -> Realtime<'_> {
        Realtime {
            shared: &self.shared,
            cursor: self.cursor.as_mut(),
        }
    }

    pub fn status(&self) -> Status {
        self.shared.status()
    }
    pub fn stats(&self) -> Stats {
        self.shared.stats()
    }

    /// Control thread only: private diagnostics may contain instrument paths or
    /// script data and are copied under a mutex, never from an audio callback.
    pub fn private_failure(&self) -> Option<String> {
        self.shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .failure
            .clone()
    }
    pub(crate) fn private_lua_failure(&self) -> Option<Arc<super::lua_failure::Context>> {
        self.shared.details.lock().unwrap_or_else(|p| p.into_inner()).lua_failure.clone()
    }
    /// Loader-control progress only: two scalars, no graph or script data.
    /// The player never retains this observer, and audio callbacks never use it.
    pub fn initialization_progress(&self) -> Option<(&'static str, Duration)> {
        let details = self
            .shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        details
            .initialization
            .current
            .map(|(phase, _)| (phase, details.initialization.started.elapsed()))
    }

    /// Cached bounded evidence for the loader and Info view. No graph traversal,
    /// Lua request, PCM copy or whole diagnostic serialization is performed.
    pub fn load_activity(&self) -> Arc<WorkerLoadActivity> {
        let mut details = self.shared.details.lock().unwrap_or_else(|p| p.into_inner());
        let status = self.status();
        if let Some((at, phase, previous_status, stages, snapshot)) = &details.activity_cache
            && at.elapsed() < Duration::from_millis(250)
            && *phase == details.phase && *previous_status == status
            && *stages == details.initialization.stages.len() {
            return snapshot.clone();
        }
        let timing = &details.initialization;
        let mut stages = timing.stages.iter().take(32).map(|&(phase, elapsed, outcome)|
            LoadStage { phase, elapsed, outcome }).collect::<Vec<_>>();
        if let Some((phase, at)) = timing.current {
            stages.push(LoadStage { phase, elapsed: at.elapsed(), outcome: "in_progress" });
        }
        let counts = details.program_report.as_ref().map(|report| &report["counts"]);
        let count = |key: &str| counts.and_then(|counts| counts[key].as_u64())
            .and_then(|n| usize::try_from(n).ok());
        let snapshot = Arc::new(WorkerLoadActivity {
            mapping: details.mapping.clone(),
            status, phase: details.phase, frame: details.phase_frame,
            elapsed: timing.finished.map_or_else(|| timing.started.elapsed(), |(elapsed, _)| elapsed),
            stages, nodes: count("nodes"), static_rejected_nodes: count("static_rejected_nodes"), sample_zones: count("sample_zones"),
            script_processors: count("script_processors"), resources: details.resource_activity.clone(),
            failure: details.failure.as_ref().map(|reason| reason.chars().take(4096).collect()),
            stats: self.stats(),
        });
        details.activity_cache = Some((Instant::now(), details.phase, status,
            details.initialization.stages.len(), snapshot.clone()));
        snapshot
    }

    /// Private control-thread inspection. A parsed/admitted graph is not proof
    /// that any particular node executed or matched Falcon numerically.
    pub fn diagnostic_report(&self) -> serde_json::Value {
        let details = self
            .shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        serde_json::json!({
            "status":self.status(), "phase":details.phase, "frame":details.phase_frame,
            "epoch":self.shared.stamp.epoch, "generation":self.shared.stamp.generation,
            "program":details.program_report.as_deref(), "stats":self.stats(),
            "initialization":details.initialization.report(),
            "failure":details.failure, "lua_failure":details.lua_failure.as_ref().map(|context|context.metadata()), "load_trace":details.load_report.as_deref(),
            "runtime_evidence":"worker_lifecycle_completed_packets_and_optional_requested_node_snapshot",
            "runtime_snapshot":details.runtime_report.as_deref(),
            "per_node_execution_proof":false, "falcon_fidelity_proof":false,
            "renderer_fidelity_caveats":details.diagnostics,
            "voice_census_semantics":"active_voices_cleared_on_stop_or_failure; last_completed_voices_historical_not_audibility",
            "lua_print_payloads_retained":false,
            "lua_print_counter_scope":"completed_render_packets_only",
        })
    }

    /// Explicit support/CLI inspection only, off UI and audio threads. Keep a
    /// stamped outcome if the request cannot settle; old cached evidence keeps
    /// its original capture stamp and is never presented as a fresh result.
    pub fn runtime_diagnostic_report(&self, timeout: Duration) -> serde_json::Value {
        let minimum = Stamp {
            frame: self.stats().processed_frame,
            ..self.shared.stamp
        };
        let mut request = None;
        let outcome = if timeout.is_zero() {
            "timeout"
        } else {
            match self.request_runtime_snapshot(minimum) {
                Err(_) => "unavailable",
                Ok(id) => {
                    request = Some(id);
                    let started = Instant::now();
                    loop {
                        if let Some(reply) = self.poll_runtime_snapshot() {
                            if reply.request == id
                                && reply.stamp.epoch == minimum.epoch
                                && reply.stamp.generation == minimum.generation
                                && reply.stamp.frame >= minimum.frame
                            {
                                break if reply.snapshot.is_ok() {
                                    "captured"
                                } else {
                                    "unavailable"
                                };
                            }
                        }
                        if self.status() != Status::Ready {
                            break "unavailable";
                        }
                        if self
                            .shared
                            .details
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .runtime_latest
                            != id
                        {
                            break "superseded";
                        }
                        if started.elapsed() >= timeout {
                            break "timeout";
                        }
                        thread::sleep(POLL.min(timeout.saturating_sub(started.elapsed())));
                    }
                }
            }
        };
        let mut report = self.diagnostic_report();
        report["runtime_snapshot_request"] = serde_json::json!({"request":request,"outcome":outcome,
            "minimum_stamp":{"epoch":minimum.epoch,"generation":minimum.generation,"frame":minimum.frame}});
        report
    }

    pub fn diagnostics(&self) -> Vec<&'static str> {
        self.shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .diagnostics
            .clone()
    }

    /// Owned initialization boundary, before audio readiness. The controller
    /// must keep gestures disabled until the audio endpoint is adopted.
    /// Lua, resource access and snapshot creation remain on this worker.
    pub fn initialized_ui(&self) -> Option<(Stamp, Arc<Vec<UiSnapshot>>)> {
        if self.shared.stop.load(Ordering::Acquire)
            || matches!(self.status(), Status::Failed | Status::Stopped)
        {
            return None;
        }
        let details = self.shared.details.lock().unwrap_or_else(|p| p.into_inner());
        details.initialized_ui.clone().map(|snapshots| (self.shared.stamp, snapshots))
    }

    /// Control thread only. Ordered processor identities for panel snapshots;
    /// contains no script source or names. Unavailable outside Ready.
    pub fn ui_processors(&self) -> Vec<NodeId> {
        if self.status() != Status::Ready {
            return Vec::new();
        }
        self.shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .ui_processors
            .clone()
    }

    /// Control thread only. Coalesces to the latest requested panel: at most one
    /// pending request and one bounded owned reply are retained. Superseded
    /// replies are discarded on this thread or the allocating playback worker.
    pub fn request_ui_snapshot(&self, processor: NodeId) -> std::result::Result<u64, PacketError> {
        self.shared.activation(self.shared.stamp)?;
        let mut details = self
            .shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let id = details.ui_latest.checked_add(1).ok_or(PacketError::Full)?;
        details.ui_latest = id;
        details.ui_request = Some(UiSnapshotRequest { id, processor });
        details.ui_reply = None;
        drop(details);
        self.shared.ui_pending.store(true, Ordering::Release);
        if let Some(handle) = &self.thread {
            handle.thread().unpark();
        }
        Ok(id)
    }

    /// Control thread only; never use this mutex or snapshot destructor in an
    /// audio callback. Match the reply's request and activation before painting.
    pub fn poll_ui_snapshot(&self) -> Option<UiSnapshotReply> {
        self.shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .ui_reply
            .take()
    }

    /// Controller only. Reject stale activation before replacing any request.
    /// expected.frame is the minimum processed boundary, never a clock advance.
    pub fn request_state_snapshot(&self, expected: Stamp) -> std::result::Result<u64, PacketError> {
        self.shared.activation(expected)?;
        let mut details = self
            .shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let id = details
            .state_latest
            .checked_add(1)
            .ok_or(PacketError::Full)?;
        details.state_latest = id;
        details.state_request = Some((id, expected.frame));
        details.state_reply = None;
        drop(details);
        self.shared.state_pending.store(true, Ordering::Release);
        if let Some(handle) = &self.thread {
            handle.thread().unpark();
        }
        Ok(id)
    }

    pub fn poll_state_snapshot(&self) -> Option<StateSnapshotReply> {
        self.shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .state_reply
            .take()
    }

    /// Control thread only. Requests a graph inspection at a real processed
    /// boundary; it never renders ahead or invokes authored callbacks.
    pub fn request_runtime_snapshot(
        &self,
        expected: Stamp,
    ) -> std::result::Result<u64, PacketError> {
        self.shared.activation(expected)?;
        let mut details = self
            .shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let id = details
            .runtime_latest
            .checked_add(1)
            .ok_or(PacketError::Full)?;
        details.runtime_latest = id;
        details.runtime_request = Some((id, expected.frame));
        details.runtime_reply = None;
        drop(details);
        self.shared.runtime_pending.store(true, Ordering::Release);
        if let Some(handle) = &self.thread {
            handle.thread().unpark();
        }
        Ok(id)
    }

    pub fn poll_runtime_snapshot(&self) -> Option<RuntimeSnapshotReply> {
        self.shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .runtime_reply
            .take()
    }

    pub fn wait_ready(&self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        loop {
            match self.status() {
                Status::Ready => return Ok(()),
                Status::Failed => anyhow::bail!(
                    "{}",
                    self.private_failure()
                        .unwrap_or_else(|| "UVI worker initialization failed".into())
                ),
                Status::Stopped => anyhow::bail!("UVI worker stopped before becoming ready"),
                Status::Starting => {}
            }
            ensure!(
                start.elapsed() < timeout,
                "UVI worker initialization timed out"
            );
            thread::sleep(POLL);
        }
    }

    /// Control thread only. Cancels even a worker waiting on a full output queue.
    /// An in-flight disk read or render must still return before the join finishes.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            handle.thread().unpark();
            if handle.join().is_err() {
                self.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
                self.shared
                    .status
                    .store(Status::Failed as u8, Ordering::Release);
            }
        }
        // Preserve the latest completed-packet voice census after stop.
        if self
            .cursor
            .as_mut()
            .is_some_and(|cursor| cursor.pending_output.take().is_some())
        {
            self.shared
                .counters
                .cancelled_outputs
                .fetch_add(1, Ordering::Relaxed);
        }
        if self
            .cursor
            .as_mut()
            .is_some_and(|cursor| cursor.pending_completion.take().is_some())
        {
            self.shared
                .counters
                .cancelled_completions
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Fixed-storage packet API: no allocation, mutex, formatting, wake syscall,
/// file access, heavyweight drop or join. Queue progress is polled by the worker.
pub struct Realtime<'a> {
    shared: &'a Shared,
    cursor: Option<&'a mut PacketCursor>,
}

impl Realtime<'_> {
    pub fn try_submit(&mut self, request: Request) -> std::result::Result<(), Rejected> {
        let Some(cursor) = self.cursor.as_mut() else {
            return Err(Rejected {
                reason: PacketError::PortTaken,
                request,
            });
        };
        let validation = self.shared.activation(request.stamp).and_then(|()| {
            if self.shared.hosted.is_some() {
                return Err(PacketError::InvalidInput);
            }
            if request.stamp.frame != cursor.next_request {
                return Err(PacketError::WrongFrame);
            }
            request.validate()
        });
        if let Err(reason) = validation {
            self.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
            return Err(Rejected { reason, request });
        }
        match self.shared.requests.push(request) {
            Ok(()) => {
                cursor.next_request += BLOCK_FRAMES as u64;
                Ok(())
            }
            Err(request) => {
                self.shared
                    .counters
                    .backpressure
                    .fetch_add(1, Ordering::Relaxed);
                Err(Rejected {
                    reason: PacketError::Full,
                    request,
                })
            }
        }
    }

    pub fn try_submit_hosted(
        &mut self,
        request: HostedRequest,
    ) -> std::result::Result<(), HostedRejected> {
        let Some(cursor) = self.cursor.as_mut() else {
            return Err(HostedRejected {
                reason: PacketError::PortTaken,
                request,
            });
        };
        let validation = self
            .shared
            .activation(request.request.stamp)
            .and_then(|()| {
                if self.shared.hosted.is_none() {
                    return Err(PacketError::InvalidInput);
                }
                if request.request.stamp.frame != cursor.next_request {
                    return Err(PacketError::WrongFrame);
                }
                request.validate()
            });
        if let Err(reason) = validation {
            self.shared.counters.errors.fetch_add(1, Ordering::Relaxed);
            return Err(HostedRejected { reason, request });
        }
        match self.shared.hosted.as_ref().unwrap().requests.push(request) {
            Ok(()) => {
                cursor.next_request += BLOCK_FRAMES as u64;
                Ok(())
            }
            Err(request) => {
                self.shared
                    .counters
                    .backpressure
                    .fetch_add(1, Ordering::Relaxed);
                Err(HostedRejected {
                    reason: PacketError::Full,
                    request,
                })
            }
        }
    }

    /// The caller supplies its current audio time. Late/stale packets are removed
    /// in at most capacity+1 steps; an early packet stays inline for a later call.
    /// Unavailable output increments `empty_output_polls` and returns the legacy
    /// `Underrun` error; the caller determines whether audio was actually due.
    /// Never substitutes old audio or silently changes the input cursor.
    pub fn try_receive(&mut self, expected: Stamp) -> std::result::Result<Output, PacketError> {
        if let Some(output) = self.try_receive_available(expected)? {
            return Ok(output);
        }
        self.shared
            .counters
            .empty_output_polls
            .fetch_add(1, Ordering::Relaxed);
        Err(PacketError::Underrun)
    }

    /// Prefetch using the same exclusive receive cursor. Empty/future packets
    /// are not audible underruns; completion delivery still uses consumed time.
    pub fn try_receive_available(
        &mut self,
        expected: Stamp,
    ) -> std::result::Result<Option<Output>, PacketError> {
        let cursor = self.cursor.as_mut().ok_or(PacketError::PortTaken)?;
        self.shared.activation(expected)?;
        if expected.frame < cursor.minimum_output
            || !expected.frame.is_multiple_of(BLOCK_FRAMES as u64)
            || expected.frame.checked_add(BLOCK_FRAMES as u64).is_none()
        {
            return Err(PacketError::WrongFrame);
        }
        cursor.minimum_output = expected.frame;
        for _ in 0..=QUEUE_CAPACITY {
            let Some(output) = cursor
                .pending_output
                .take()
                .or_else(|| self.shared.outputs.pop())
            else {
                break;
            };
            if output.stamp.epoch != expected.epoch
                || output.stamp.generation != expected.generation
                || output.stamp.frame < expected.frame
            {
                self.shared
                    .counters
                    .stale_packets
                    .fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if output.stamp.frame > expected.frame {
                cursor.pending_output = Some(output);
                break;
            }
            cursor.minimum_output += BLOCK_FRAMES as u64;
            return Ok(Some(output));
        }
        Ok(None)
    }

    /// The caller supplies the currently consumed boundary on this worker's
    /// activation timeline, mapping host time through the admitted buffering
    /// latency. An exclusive renderer end becomes eligible at that boundary.
    /// Late completions survive associated PCM discard.
    /// Empty/future queues are normal; stale removal is bounded by capacity+1.
    pub fn try_receive_completion(
        &mut self,
        expected: Stamp,
    ) -> std::result::Result<Option<StampedCompletion>, PacketError> {
        let cursor = self.cursor.as_mut().ok_or(PacketError::PortTaken)?;
        self.shared.activation(expected)?;
        let hosted = self
            .shared
            .hosted
            .as_ref()
            .ok_or(PacketError::InvalidInput)?;
        if expected.frame < cursor.completion_boundary {
            return Err(PacketError::WrongFrame);
        }
        cursor.completion_boundary = expected.frame;
        for _ in 0..=COMPLETION_CAPACITY {
            let Some(completion) = cursor
                .pending_completion
                .take()
                .or_else(|| hosted.completions.pop())
            else {
                return Ok(None);
            };
            if completion.root.token == 0
                || completion.stamp.epoch != expected.epoch
                || completion.stamp.generation != expected.generation
                || completion.root.epoch != completion.stamp.epoch
                || completion.root.generation != completion.stamp.generation
            {
                self.shared
                    .counters
                    .stale_packets
                    .fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if completion.stamp.frame > expected.frame {
                cursor.pending_completion = Some(completion);
                return Ok(None);
            }
            return Ok(Some(completion));
        }
        Ok(None)
    }

    pub fn status(&self) -> Status {
        self.shared.status()
    }
    pub fn stats(&self) -> Stats {
        self.shared.stats()
    }
}

fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn capture_ui(player: &Player<'_>, shared: &Shared) {
    if !shared.ui_pending.load(Ordering::Acquire)
        || !shared.ui_pending.swap(false, Ordering::AcqRel)
    {
        return;
    }
    let request = shared
        .details
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .ui_request
        .take();
    let Some(request) = request else { return };
    let reply = UiSnapshotReply {
        request: request.id,
        stamp: Stamp {
            frame: player.current_frame(),
            ..shared.stamp
        },
        processor: request.processor,
        snapshot: player
            .ui_snapshot(request.processor)
            .map_err(|_| UiSnapshotError::Unavailable),
    };
    let mut details = shared
        .details
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if details.ui_latest == request.id && !shared.stop.load(Ordering::Acquire) {
        details.ui_reply = Some(reply);
    }
}

fn capture_state(player: &mut Player<'_>, shared: &Shared) -> Result<()> {
    if !shared.state_pending.swap(false, Ordering::AcqRel) {
        return Ok(());
    }
    let request = {
        let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
        let Some((request, minimum)) = details.state_request else {
            return Ok(());
        };
        if player.current_frame() < minimum {
            shared.state_pending.store(true, Ordering::Release);
            return Ok(());
        }
        details.state_request = None;
        request
    };
    phase(shared, "state_capture", player.current_frame());
    let captured = player.saved_state();
    let (snapshot, failure) = match captured {
        Ok(state) => (Ok(state), None),
        Err(error) => (Err(StateSnapshotError::Execution), Some(error)),
    };
    let reply = StateSnapshotReply {
        request,
        stamp: Stamp {
            frame: player.current_frame(),
            ..shared.stamp
        },
        snapshot,
    };
    let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
    if details.state_latest == request && !shared.stop.load(Ordering::Acquire) {
        details.state_reply = Some(reply);
    }
    drop(details);
    if let Some(error) = failure {
        return Err(error.context("Native state capture failed; activation aborted"));
    }
    Ok(())
}

fn capture_runtime(player: &Player<'_>, shared: &Shared) {
    if !shared.runtime_pending.swap(false, Ordering::AcqRel) {
        return;
    }
    let request = {
        let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
        let Some((request, minimum)) = details.runtime_request else {
            return;
        };
        if player.current_frame() < minimum {
            shared.runtime_pending.store(true, Ordering::Release);
            return;
        }
        details.runtime_request = None;
        request
    };
    let stamp = Stamp {
        frame: player.current_frame(),
        ..shared.stamp
    };
    phase(shared, "runtime_snapshot", stamp.frame);
    let snapshot = serde_json::to_value(player.runtime_evidence())
        .map(|evidence| {
            Arc::new(serde_json::json!({"stamp":{"epoch":stamp.epoch,
            "generation":stamp.generation,"frame":stamp.frame},"evidence":evidence}))
        })
        .map_err(|_| RuntimeSnapshotError::Unavailable);
    let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
    if details.runtime_latest == request && !shared.stop.load(Ordering::Acquire) {
        if let Ok(report) = &snapshot {
            details.runtime_report = Some(report.clone());
        }
        details.runtime_reply = Some(RuntimeSnapshotReply {
            request,
            stamp,
            snapshot,
        });
    }
}

/// Controller observes an activation abort, not successful ends for discarded
/// roots. A future adapter must retire canonical owners through its lifecycle.
fn finish(shared: &Shared, failure: Option<String>) {
    shared.counters.active_voices.store(0, Ordering::Relaxed);
    // Never retain an initialized panel after its worker fails or is cancelled.
    shared.details.lock().unwrap_or_else(|p| p.into_inner()).initialized_ui = None;
    // Stats retain the census from the latest completed packet after failure/stop.
    if let Some(failure) = failure {
        shared.counters.errors.fetch_add(1, Ordering::Relaxed);
        shared
            .details
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .failure = Some(failure);
        shared.status.store(Status::Failed as u8, Ordering::Release);
    } else {
        shared
            .status
            .store(Status::Stopped as u8, Ordering::Release);
    }
    while shared.requests.pop().is_some() {
        shared
            .counters
            .cancelled_requests
            .fetch_add(1, Ordering::Relaxed);
    }
    while shared.outputs.pop().is_some() {
        shared
            .counters
            .cancelled_outputs
            .fetch_add(1, Ordering::Relaxed);
    }
    if let Some(hosted) = &shared.hosted {
        while hosted.requests.pop().is_some() {
            shared
                .counters
                .cancelled_requests
                .fetch_add(1, Ordering::Relaxed);
        }
        while hosted.completions.pop().is_some() {
            shared
                .counters
                .cancelled_completions
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// EndReady is an authoritative retained census until acknowledgement. Verify
/// every old untransferred suffix record survives unchanged before replacement.
fn refresh_completions(pending: &mut Vec<HostCompletion>, next: Vec<HostCompletion>) -> Result<()> {
    use std::collections::HashMap;
    ensure!(
        next.len() <= COMPLETION_CAPACITY,
        "Hosted completion census exceeds capacity"
    );
    let mut census = HashMap::with_capacity(next.len());
    for completion in &next {
        ensure!(
            census.insert(completion.root, completion.frame).is_none(),
            "Duplicate hosted completion"
        );
    }
    ensure!(
        pending
            .iter()
            .all(|old| census.get(&old.root) == Some(&old.frame)),
        "Untransferred hosted completion missing or changed"
    );
    *pending = next;
    Ok(())
}

/// Only called on the allocating playback worker. Durable queue transfer
/// precedes each ledger acknowledgement; a full queue retains the untouched
/// suffix for retries, including when no new audio request arrives.
fn transfer_completions(
    player: &mut Player<'_>,
    shared: &Shared,
    pending: &mut Vec<HostCompletion>,
) -> Result<usize> {
    let hosted = shared
        .hosted
        .as_ref()
        .context("Worker has no hosted transport")?;
    ensure!(
        shared.status() != Status::Failed,
        "Hosted activation already aborted"
    );
    ensure!(
        pending.len() <= COMPLETION_CAPACITY,
        "Hosted completion batch exceeds capacity"
    );
    ensure!(
        pending
            .iter()
            .all(|completion| completion.root.epoch == shared.stamp.epoch
                && completion.root.generation == shared.stamp.generation
                && completion.root.token > 0
                && completion.frame <= player.current_frame()),
        "Invalid hosted completion stamp"
    );
    ensure!(
        pending
            .windows(2)
            .all(|pair| pair[0].frame <= pair[1].frame),
        "Hosted completions are not ordered"
    );
    let mut roots = std::collections::HashSet::with_capacity(pending.len());
    ensure!(
        pending
            .iter()
            .all(|completion| roots.insert(completion.root)),
        "Duplicate hosted completion"
    );
    let mut transferred = 0;
    while transferred < pending.len() && !shared.stop.load(Ordering::Acquire) {
        let completion = pending[transferred];
        let packet = StampedCompletion {
            stamp: Stamp {
                frame: completion.frame,
                ..shared.stamp
            },
            root: completion.root,
        };
        if hosted.completions.push(packet).is_err() {
            break;
        }
        // Once durably pushed, never retry this record, even if the ledger
        // acknowledgement fails. Abort this activation instead of duplicating.
        transferred += 1;
        if let Err(error) = player.acknowledge_host_completions(&[completion.root]) {
            pending.drain(..transferred);
            shared.status.store(Status::Failed as u8, Ordering::Release);
            return Err(
                error.context("Hosted completion acknowledgement failed; activation aborted")
            );
        }
    }
    pending.drain(..transferred);
    Ok(transferred)
}

fn run(
    config: StartConfig,
    shared: &Shared,
    initialized: Instant,
    saved: Option<&super::state::SavedState>,
    trace: &mut crate::diagnostics::LoadTrace,
) -> Result<()> {
    // Everything containing Rc, borrowed graph nodes, Lua or file authority is
    // created, used and destroyed in this stack frame on this dedicated thread.
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    initialization_stage(shared, trace, "uvi_bank_open");
    let library = Rc::new(Library::open(
        &config.bank,
        &config.metadata_namespace,
        config.content_key,
    )?);
    ensure!(
        config
            .expected_bank_uuid
            .is_none_or(|uuid| uuid == library.bank.header.uuid),
        "UVI bank identity changed before worker initialization"
    );
    if let Some(identity) = &config.content_bank {
        ensure!(
            identity == &library.bank.header.bank_name
                || std::fs::canonicalize(identity).ok()
                    == Some(std::fs::canonicalize(&config.bank)?),
            "Content state belongs to a different bank"
        );
    }
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    initialization_stage(shared, trace, "uvi_program_decode");
    let loaded = library.program(&config.member, &config.program_namespace)?;
    initialization_stage(shared, trace, "uvi_graph_diagnosis_and_preflight");
    let preflight = super::playback::ProgramPreflight::new(&loaded.program);
    let report = Arc::new(serde_json::to_value(super::diagnostics::report_preflighted(&preflight))?);
    trace.detail("native_program_graph", report.as_ref().clone());
    {
        let mapping = Arc::new(super::mapping::Inspection::parsed(shared.stamp, &loaded.program, report.clone()));
        let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
        details.program_report = Some(report);
        details.mapping = Some(mapping);
        details.activity_cache = None;
    }
    initialization_stage(shared, trace, "uvi_preflight");
    preflight.validate()?;
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    initialization_stage(shared, trace, "uvi_resources");
    let samples = match library.samples_with_progress_cancel(&loaded, &mut |total, loaded, unique_decodes, bytes, current| {
        let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
        details.resource_activity = ResourceActivity {
            total: Some(total), loaded, unique_decodes, bytes,
            current: current.map(|path| path.chars().take(256).collect()),
        };
    }, Some(&shared.stop)) {
        Ok(samples) => samples,
        Err(error) if error.is::<super::sample::LoadCancelled>() => return Ok(()),
        Err(error) => return Err(error),
    };
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    {
        let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
        details.mapping = details.mapping.as_ref().map(|mapping| Arc::new(mapping.decoded(&samples)));
        details.activity_cache = None;
    }
    let resources = BankResources::new(library.clone(), &loaded.path, samples)?;
    let activation = shared
        .hosted
        .as_ref()
        .map(|_| (shared.stamp.epoch, shared.stamp.generation));
    initialization_stage(shared, trace, "uvi_modules");
    let modules = library.modules()?;
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut player = match Player::new_with_state_traced(
        &preflight,
        modules,
        resources,
        config.sample_rate,
        activation,
        saved,
        &mut |name| initialization_stage(shared, trace, name),
        &mut |session| {
            if shared.stop.load(Ordering::Acquire) { return; }
            // The normal Ready mailbox remains available for larger chains.
            // Do not let one eager batch amplify per-processor UI budgets.
            let processors: Vec<_> = loaded.program.nodes.iter().enumerate()
                .filter_map(|(id, node)| (node.kind == "ScriptProcessor").then_some(id))
                .take(65).collect();
            if processors.len() > 64 { return; }
            let mut snapshots = Vec::new();
            let mut widgets = 0usize;
            for processor in processors {
                if shared.stop.load(Ordering::Acquire) { return; }
                if let Ok(snapshot) = session.ui_snapshot(processor) {
                    widgets += snapshot.widgets.len();
                    if widgets > 16_384 { return; }
                    snapshots.push(snapshot);
                }
            }
            let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
            if !shared.stop.load(Ordering::Acquire) {
                details.initialized_ui = Some(Arc::new(snapshots));
            }
        },
        Some(&shared.stop),
    ) {
        Ok(player) => player,
        Err(error) if error.is::<super::sample::LoadCancelled>() => return Ok(()),
        Err(error) => return Err(retain_lua_failure(shared, error, &loaded.program)),
    };
    {
        let mut details = shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        details.diagnostics = player.diagnostics();
        details.ui_processors = loaded
            .program
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| (node.kind == "ScriptProcessor").then_some(id))
            .collect();
    }
    shared
        .counters
        .initialization_ns
        .store(nanos(initialized.elapsed()).max(1), Ordering::Relaxed);
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
    shared
        .details
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .initialization
        .finish("ready");
    phase(shared, "serve", 0);
    trace.stage("uvi_serve");
    shared.status.store(Status::Ready as u8, Ordering::Release);
    serve(&mut player, shared, config.sample_rate)
        .map_err(|error| retain_lua_failure(shared, error, &loaded.program))
}

fn retain_lua_failure(shared: &Shared, error: anyhow::Error, program: &super::program::Program) -> anyhow::Error {
    if let Some(context) = super::lua_failure::from_error(&error, program) {
        shared.details.lock().unwrap_or_else(|p| p.into_inner()).lua_failure = Some(Arc::new(context));
    }
    error
}

fn initialization_stage(
    shared: &Shared,
    trace: &mut crate::diagnostics::LoadTrace,
    name: &'static str,
) {
    let phase = name.strip_prefix("uvi_").unwrap_or(name);
    {
        let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
        details.phase = phase;
        details.phase_frame = 0;
        details.initialization.stage(phase);
    }
    trace.stage(name);
}

fn phase(shared: &Shared, name: &'static str, frame: u64) {
    let mut details = shared.details.lock().unwrap_or_else(|p| p.into_inner());
    details.phase = name;
    details.phase_frame = frame;
}

fn serve(player: &mut Player<'_>, shared: &Shared, sample_rate: u32) -> Result<()> {
    let deadline = Duration::from_secs_f64(BLOCK_FRAMES as f64 / f64::from(sample_rate));
    let mut pending_completions = shared
        .hosted
        .as_ref()
        .map(|_| Vec::with_capacity(COMPLETION_CAPACITY));
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(pending) = &mut pending_completions {
            transfer_completions(player, shared, pending)?;
        }
        capture_ui(&player, shared);
        capture_state(player, shared)?;
        capture_runtime(player, shared);
        phase(shared, "serve", player.current_frame());
        let packet = if let Some(hosted) = &shared.hosted {
            hosted
                .requests
                .pop()
                .map(|packet| (packet.request, Some(packet)))
        } else {
            shared.requests.pop().map(|request| (request, None))
        };
        let Some((request, hosted)) = packet else {
            // ponytail: 1 ms bounded polling avoids audio-thread wake/lock calls;
            // replace it only after a measured transport deadline requires it.
            thread::park_timeout(POLL);
            continue;
        };
        ensure!(
            request.stamp.epoch == shared.stamp.epoch
                && request.stamp.generation == shared.stamp.generation
                && request.stamp.frame == player.current_frame(),
            "UVI worker request stamp is out of order"
        );
        phase(shared, "packet_render", request.stamp.frame);
        let started = Instant::now();
        // A queued authoritative-lifetime rejection is fatal: return before
        // rendering or advancing this block; finish() aborts the activation.
        request.validate()?;
        let rendered = if let Some(packet) = hosted {
            packet.validate()?;
            player.render_hosted_with_ui(
                &request.inputs[..usize::from(request.input_count)],
                &request.ui_inputs[..usize::from(request.ui_count)],
                &packet.roots[..usize::from(packet.root_count)],
                BLOCK_FRAMES,
            )
        } else {
            player.render_with_ui(
                &request.inputs[..usize::from(request.input_count)],
                &request.ui_inputs[..usize::from(request.ui_count)],
                BLOCK_FRAMES,
            )
        };
        let elapsed = started.elapsed();
        let duration = nanos(elapsed);
        shared
            .counters
            .render_ns
            .fetch_add(duration, Ordering::Relaxed);
        shared
            .counters
            .max_render_ns
            .fetch_max(duration, Ordering::Relaxed);
        if elapsed > deadline {
            shared
                .counters
                .render_deadline_misses
                .fetch_add(1, Ordering::Relaxed);
        }
        let rendered = rendered
            .with_context(|| format!("UVI packet rendering at frame {}", request.stamp.frame))?;
        shared
            .counters
            .processed_frame
            .store(player.current_frame(), Ordering::Relaxed);
        shared
            .counters
            .logs
            .fetch_add(rendered.logs.len() as u64, Ordering::Relaxed);
        shared
            .counters
            .dropped_logs
            .fetch_add(rendered.dropped_logs as u64, Ordering::Relaxed);
        if let Some(pending) = &mut pending_completions {
            refresh_completions(pending, rendered.host_completions)?;
            transfer_completions(player, shared, pending)?;
        }
        ensure!(
            rendered.audio.len() == BLOCK_FRAMES,
            "UVI worker renderer returned a short packet"
        );
        let mut output = Output {
            stamp: request.stamp,
            audio: [[0.; 2]; BLOCK_FRAMES],
            commands: rendered.commands as u32,
            host_commands: rendered.host_commands as u32,
            logs: rendered.logs.len() as u32,
            dropped_logs: rendered.dropped_logs as u32,
            rejected_ui: rendered.rejected_ui,
        };
        output.audio.copy_from_slice(&rendered.audio);
        shared
            .counters
            .active_voices
            .store(player.active_voices() as u64, Ordering::Relaxed);
        shared
            .counters
            .last_completed_voices
            .store(player.active_voices() as u64, Ordering::Relaxed);
        shared
            .counters
            .rendered_blocks
            .fetch_add(1, Ordering::Relaxed);
        loop {
            if shared.stop.load(Ordering::Acquire) {
                shared
                    .counters
                    .cancelled_outputs
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(());
            }
            if let Some(pending) = &mut pending_completions {
                transfer_completions(player, shared, pending)?;
            }
            capture_ui(&player, shared);
            capture_state(player, shared)?;
            capture_runtime(player, shared);
            phase(shared, "serve", player.current_frame());
            match shared.outputs.push(output) {
                Ok(()) => break,
                Err(pending) => {
                    output = pending;
                    thread::park_timeout(POLL);
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{crypto, program::parse_program};
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn initial_player_stops_between_ui_observer_and_renderer() {
        let (config, _) = authored_bank_with_script("function onInit()end");
        let path = config.bank.clone();
        let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
        let loaded = library.program(&config.member, &config.program_namespace).unwrap();
        let preflight = super::super::playback::ProgramPreflight::new(&loaded.program);
        let stop = AtomicBool::new(false);
        let mut stages = Vec::new();
        let result = Player::new_with_state_traced(&preflight, BTreeMap::new(),
            BankResources::new(library, &loaded.path, Default::default()).unwrap(),
            48000, None, None, &mut |name| stages.push(name),
            &mut |_| stop.store(true, Ordering::Release), Some(&stop));
        assert!(result.err().unwrap().is::<super::super::sample::LoadCancelled>());
        assert_eq!(stages, ["uvi_player_preflight", "uvi_lua_init"]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn initial_player_stops_after_lua_and_preserves_real_lua_failure() {
        for fails in [false, true] {
            let (config, _) = authored_bank_with_script(if fails {
                "function onInit()error('authored cancellation race')end"
            } else { "function onInit()knob=Knob('initialized',0.5,0,1)end" });
            let path = config.bank.clone();
            let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
            let loaded = library.program(&config.member, &config.program_namespace).unwrap();
            let preflight = super::super::playback::ProgramPreflight::new(&loaded.program);
            let stop = AtomicBool::new(false);
            let mut stages = Vec::new();
            let published = std::cell::Cell::new(false);
            let result = Player::new_with_state_traced(&preflight, BTreeMap::new(),
                BankResources::new(library, &loaded.path, Default::default()).unwrap(),
                48000, None, None, &mut |name| {
                    stages.push(name);
                    if name == "uvi_lua_init" { stop.store(true, Ordering::Release); }
                }, &mut |_| published.set(true), Some(&stop));
            let error = result.err().unwrap();
            assert_eq!(error.is::<super::super::sample::LoadCancelled>(), !fails);
            if fails { assert!(format!("{error:#}").contains("authored cancellation race")); }
            assert!(!published.get());
            assert_eq!(stages, ["uvi_player_preflight", "uvi_lua_init"]);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn initialized_ui_is_owned_stamped_and_cleared_on_failure_or_stop() {
        for failure in [None, Some("authored terminal failure".to_owned())] {
            let worker = Worker { shared: Arc::new(Shared::new(7, 11)), thread: None,
                cursor: Some(PacketCursor::default()) };
            let initialized = Arc::new(Vec::new());
            worker.shared.details.lock().unwrap().initialized_ui = Some(initialized.clone());
            let (stamp, captured) = worker.initialized_ui().unwrap();
            assert_eq!((stamp.epoch, stamp.generation, stamp.frame), (7, 11, 0));
            assert!(Arc::ptr_eq(&captured, &initialized));
            assert_eq!(worker.status(), Status::Starting);
            assert!(worker.ui_processors().is_empty());
            worker.shared.stop.store(true, Ordering::Release);
            assert!(worker.initialized_ui().is_none());
            finish(&worker.shared, failure);
            assert!(worker.shared.details.lock().unwrap().initialized_ui.is_none());
            assert!(worker.initialized_ui().is_none());
        }
    }

    #[test]
    fn initialization_budget_context_stays_local_after_worker_failure() {
        let (config,_) = authored_bank_with_script("-- private-worker-budget-source-only-marker\nfor i=1,10000000 do local x=i+i end");
        let path=config.bank.clone();
        let worker=Worker::start(config,7,11).unwrap();
        assert!(worker.wait_ready(Duration::from_secs(3)).is_err());
        let local=worker.private_lua_failure().unwrap();
        assert_eq!((local.processor,local.frame,local.line),(Some(2),0,Some(2)));
        assert_eq!(local.provenance,"existing_instruction_budget_hook");
        assert!(local.display.contains("private-worker-budget-source-only-marker"));
        let report=worker.diagnostic_report();
        assert!(!report.to_string().contains("private-worker-budget-source-only-marker"));
        assert_eq!(report["lua_failure"]["source_provenance"],"existing_instruction_budget_hook");
        assert!(worker.initialized_ui().is_none());
        drop(worker);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn authored_initialization_failure_never_publishes_partial_ui() {
        let (config, _) = authored_bank_with_script(
            "function onInit()knob=Knob('partial',0.5,0,1);error('authored initialization failure')end");
        let path = config.bank.clone();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        assert!(worker.wait_ready(Duration::from_secs(5)).is_err());
        assert_eq!(worker.status(), Status::Failed);
        assert!(worker.initialized_ui().is_none());
        assert_eq!(worker.stats().rendered_blocks, 0);
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn renderer_failure_retires_an_initialized_panel_without_audio() {
        let (fixture, source) = authored_bank_with_script(
            "function onInit()knob=Knob('initialized',0.5,0,1)end");
        std::fs::remove_file(fixture.bank).unwrap();
        // Generator parameters pass graph preflight, but renderer ownership
        // validation rejects this oscillator outside a Keygroup.
        let source = source.replace("<Layers><Layer><Keygroups><Keygroup><Oscillators>", "<Oscillators>")
            .replace("</Oscillators></Keygroup></Keygroups></Layer></Layers>", "</Oscillators>");
        let (config, _) = authored_bank_with_program(&source);
        let path = config.bank.clone();
        let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
        let loaded = library.program(&config.member, &config.program_namespace).unwrap();
        let published = std::cell::Cell::new(false);
        let result = Player::new_with_state_traced(&super::super::playback::ProgramPreflight::new(&loaded.program), BTreeMap::new(),
            BankResources::new(library, &loaded.path, Default::default()).unwrap(),
            48000, None, None, &mut |_| {}, &mut |_| published.set(true), None);
        assert!(published.get());
        assert!(result.is_err());
        let mut worker = Worker::start(config, 7, 11).unwrap();
        assert!(worker.wait_ready(Duration::from_secs(5)).is_err());
        assert!(worker.initialized_ui().is_none());
        assert_eq!(worker.stats().rendered_blocks, 0);
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn initialized_ui_follows_authored_lua_and_preserves_restore_apply_order() {
        use super::super::host::UiValue;
        let (config, _) = authored_bank_with_script(r#"
            knob=Knob{name='authored',value=0.25}
            function onInit()knob.value=0.75;Program:setParameter('Gain',0.6)end
            function onSave()return {saved=true}end
            function onLoad(data)assert(data.saved);Program:setParameter('Gain',0.7)end
        "#);
        let path = config.bank.clone();
        let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
        let loaded = library.program(&config.member, &config.program_namespace).unwrap();
        let processor = loaded.program.nodes.iter().position(|n| n.kind == "ScriptProcessor").unwrap();
        let phase = std::cell::Cell::new("");
        let seen = std::cell::RefCell::new(Vec::new());
        let mut fresh = Player::new_with_state_traced(&super::super::playback::ProgramPreflight::new(&loaded.program), BTreeMap::new(),
            BankResources::new(library.clone(), &loaded.path, Default::default()).unwrap(),
            48000, Some((7,11)), None, &mut |name|phase.set(name), &mut |session| {
                seen.borrow_mut().push(phase.get());
                assert!(matches!(session.ui_snapshot(processor).unwrap().widgets[0].value,
                    Some(UiValue::Number(n)) if n==0.75));
            }, None).unwrap();
        assert_eq!(*seen.borrow(), vec!["uvi_lua_init"]);
        let saved = fresh.saved_state().unwrap();
        seen.borrow_mut().clear();
        let mut restored = Player::new_with_state_traced(&super::super::playback::ProgramPreflight::new(&loaded.program), BTreeMap::new(),
            BankResources::new(library, &loaded.path, Default::default()).unwrap(),
            48000, Some((7,12)), Some(&saved), &mut |name|phase.set(name), &mut |session| {
                seen.borrow_mut().push(phase.get());
                assert!(session.ui_snapshot(processor).unwrap() == fresh.ui_snapshot(processor).unwrap());
            }, None).unwrap();
        assert_eq!(*seen.borrow(), vec!["uvi_restore_apply"]);
        assert!(restored.saved_state().is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn live_load_activity_is_cached_bounded_and_refreshes_on_stage_or_failure() {
        let worker = Worker { shared: Arc::new(Shared::new(1, 2)), thread: None,
            cursor: Some(PacketCursor::default()) };
        let first = worker.load_activity();
        assert!(Arc::ptr_eq(&first, &worker.load_activity()));
        assert_eq!(first.static_rejected_nodes, None);
        {
            let mut details = worker.shared.details.lock().unwrap();
            details.phase = "resources";
            details.program_report = Some(Arc::new(serde_json::json!({"counts":{"nodes":3,"static_rejected_nodes":1}})));
            details.initialization.stage("resources");
            details.resource_activity = ResourceActivity { total: Some(4), loaded: 2,
                unique_decodes: 1, bytes: 4096, current: Some("Samples/test.wav".into()) };
        }
        let loading = worker.load_activity();
        assert!(!Arc::ptr_eq(&first, &loading));
        assert_eq!((loading.resources.total, loading.resources.loaded, loading.resources.bytes), (Some(4), 2, 4096));
        assert_eq!(loading.stages.last().unwrap().outcome, "in_progress");
        assert_eq!(loading.static_rejected_nodes, Some(1));
        {
            let mut details = worker.shared.details.lock().unwrap();
            details.initialization.finish("failed");
            details.failure = Some("authored error".repeat(1000));
            assert!(details.ui_request.is_none() && details.state_request.is_none() && details.runtime_request.is_none());
        }
        worker.shared.status.store(Status::Failed as u8, Ordering::Release);
        let failed = worker.load_activity();
        assert!(!Arc::ptr_eq(&loading, &failed));
        assert_eq!(failed.stages.last().unwrap().outcome, "failed");
        assert_eq!(failed.failure.as_ref().unwrap().chars().count(), 4096);
        let elapsed = failed.elapsed;
        worker.shared.details.lock().unwrap().activity_cache = None;
        assert_eq!(worker.load_activity().elapsed, elapsed);
    }

    #[test]
    fn compact_initialization_progress_does_not_enqueue_or_copy_graph_reports() {
        let worker = Worker {
            shared: Arc::new(Shared::new(1, 2)),
            thread: None,
            cursor: Some(PacketCursor::default()),
        };
        {
            let mut details = worker.shared.details.lock().unwrap();
            details.initialization.started = Instant::now() - Duration::from_secs(3);
            details.initialization.stage("resources");
        }
        let (phase, elapsed) = worker.initialization_progress().unwrap();
        assert_eq!(phase, "resources");
        assert!(
            elapsed >= Duration::from_secs(3),
            "elapsed tracks complete initialization, not just current phase"
        );
        {
            let details = worker.shared.details.lock().unwrap();
            assert!(
                details.ui_request.is_none()
                    && details.state_request.is_none()
                    && details.runtime_request.is_none()
            );
            assert!(details.program_report.is_none());
        }
        worker
            .shared
            .details
            .lock()
            .unwrap()
            .initialization
            .finish("ready");
        assert!(worker.initialization_progress().is_none());
    }

    #[test]
    fn initialization_timing_reports_live_stage_and_freezes_terminal_duration() {
        let mut timing = InitializationTiming::default();
        timing.stage("resources");
        let loading = timing.report();
        assert_eq!(loading["outcome"], "in_progress");
        assert_eq!(loading["stages"][0]["phase"], "starting");
        assert_eq!(loading["stages"][0]["outcome"], "finished");
        assert_eq!(loading["stages"][1]["phase"], "resources");
        assert_eq!(loading["stages"][1]["outcome"], "in_progress");
        timing.finish("failed");
        let terminal = timing.report();
        timing.stage("serve");
        timing.finish("cancelled");
        assert_eq!(
            timing.report(),
            terminal,
            "later stop/poll cannot inflate failed initialization time"
        );
        assert_eq!(terminal["outcome"], "failed");
        assert_eq!(terminal["stages"][1]["outcome"], "failed");
        assert_eq!(terminal["ui_assets_measured"], false);
        assert_eq!(terminal["host_adoption_wait_measured"], false);
    }

    #[test]
    fn initialization_timing_splits_player_and_preserves_failed_lua_phase() {
        let (config, _) = authored_bank_with_script(
            "function onInit()error('authored initialization failure')end",
        );
        let path = config.bank.clone();
        let mut worker = Worker::start_hosted(config, 3, 4).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.status() == Status::Starting && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        assert_eq!(worker.status(), Status::Failed);
        let report = worker.diagnostic_report();
        assert_eq!(report["phase"], "lua_init");
        let timing = &report["initialization"];
        assert_eq!(timing["outcome"], "failed");
        let stages = timing["stages"].as_array().unwrap();
        for name in [
            "bank_open",
            "program_decode",
            "graph_diagnosis_and_preflight",
            "preflight",
            "resources",
            "modules",
            "player_preflight",
        ] {
            assert!(
                stages
                    .iter()
                    .any(|stage| stage["phase"] == name && stage["outcome"] == "finished"),
                "missing completed phase {name}"
            );
        }
        assert_eq!(stages.last().unwrap()["phase"], "lua_init");
        assert_eq!(stages.last().unwrap()["outcome"], "failed");
        assert!(
            report["failure"]
                .as_str()
                .unwrap()
                .contains("authored initialization failure")
        );
        assert_eq!(report["load_trace"]["details"]["failure_phase"], "lua_init");
        worker.stop();
        assert_eq!(worker.diagnostic_report()["initialization"], *timing);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn diagnostic_report_retains_decoded_graph_when_unknown_node_fails_preflight() {
        let (config, _) = authored_bank_with_script(
            "]]></script></ScriptProcessor><UnsupportedDiagnosticNode/><ScriptProcessor><script><![CDATA[",
        );
        let path = config.bank.clone();
        let worker = Worker::start(config, 7, 11).unwrap();
        assert!(worker.wait_ready(Duration::from_secs(3)).is_err());
        let report = worker.diagnostic_report();
        assert_eq!(report["status"], "failed");
        assert_eq!(report["phase"], "preflight");
        assert_eq!(report["program"]["parsed"], true);
        assert_eq!(report["program"]["preflight_admitted"], false);
        assert!(
            report["program"]["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["kind"] == "UnsupportedDiagnosticNode")
        );
        assert_eq!(report["stats"]["rendered_blocks"], 0);
        assert_eq!(report["per_node_execution_proof"], false);
        assert!(
            report["failure"]
                .as_str()
                .unwrap()
                .contains("UnsupportedDiagnosticNode")
        );
        assert_eq!(
            report["load_trace"]["details"]["failure_phase"],
            "preflight"
        );
        drop(worker);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn runtime_diagnostic_preserves_script_node_original_line_frame_and_latest_packet_stats() {
        let (config, _) = authored_bank_with_script(
            "counter=0\nfunction onNote(e)\n if counter==1 then error('runtime-private-marker') end\n counter=counter+1\n print('private-print-marker')\n postEvent(e)\nend",
        );
        let path = config.bank.clone();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(3)).unwrap();
        worker
            .realtime()
            .try_submit(Request::new(stamp(0), &[note(0)]).unwrap())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if worker
                .realtime()
                .try_receive_available(stamp(0))
                .unwrap()
                .is_some()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let before = worker.stats();
        assert_eq!(before.rendered_blocks, 1);
        assert!(before.active_voices > 0);
        worker
            .realtime()
            .try_submit(Request::new(stamp(256), &[note(256)]).unwrap())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.status() != Status::Failed {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let report = worker.diagnostic_report();
        assert_eq!(report["phase"], "packet_render");
        assert_eq!(report["frame"], 256);
        let failure = report["failure"].as_str().unwrap();
        assert!(
            failure.contains("UVI ScriptProcessor node 2")
                && failure.contains(":3:")
                && failure.contains("runtime-private-marker"),
            "{failure}"
        );
        assert_eq!(report["stats"]["rendered_blocks"], 1);
        assert_eq!(report["stats"]["processed_frame"], 256);
        assert_eq!(report["stats"]["active_voices"], 0);
        assert_eq!(
            report["stats"]["last_completed_voices"],
            before.active_voices
        );
        assert_eq!(report["stats"]["logs"], 1);
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("private-print-marker")
        );
        assert_eq!(report["load_trace"]["details"]["failure_frame"], 256);
        let local=worker.private_lua_failure().unwrap();
        assert_eq!((local.processor,local.frame,local.line),(Some(2),256,Some(3)));
        assert!(local.display.contains("if counter==1 then error("));
        assert!(!report.to_string().contains("if counter==1 then error("),"commercial code remains absent from serialized report/export context");
        assert_eq!(report["lua_failure"]["local_source_excerpt_available"],true);

        drop(worker);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stopped_report_retains_last_completed_packet_voice_census_and_boundary() {
        let (config, _) = authored_bank_with_script("function onNote(e)postEvent(e)end");
        let path = config.bank.clone();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(3)).unwrap();
        worker
            .realtime()
            .try_submit(Request::new(stamp(0), &[note(0)]).unwrap())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if worker
                .realtime()
                .try_receive_available(stamp(0))
                .unwrap()
                .is_some()
            {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        let latest = worker.stats();
        assert!(latest.active_voices > 0);
        worker.stop();
        let report = worker.diagnostic_report();
        assert_eq!(report["status"], "stopped");
        assert_eq!(report["phase"], "serve");
        assert_eq!(report["stats"]["active_voices"], 0);
        assert_eq!(
            report["stats"]["last_completed_voices"],
            latest.active_voices
        );
        assert_eq!(report["stats"]["rendered_blocks"], 1);
        assert_eq!(report["stats"]["processed_frame"], 256);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn requested_runtime_snapshot_waits_for_boundary_tracks_silent_voices_and_keeps_unknown_nodes_honest()
     {
        let (config, _) = authored_bank_with_program(
            r#"<Program Gain="0"><EventProcessors><ScriptProcessor><script><![CDATA[
function onNote(e)postEvent(e)end
function onSave()error('runtime inspection must not run callbacks')end
]]></script></ScriptProcessor></EventProcessors><Inserts><Gain Bypass="1" Volume="0.5"/><Gain Volume="0.5"/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" StartPhase="0.25" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#,
        );
        let path = config.bank.clone();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(3)).unwrap();
        assert!(
            worker.diagnostic_report()["runtime_snapshot"].is_null(),
            "ordinary initialization does not traverse runtime evidence"
        );
        let first = worker.request_runtime_snapshot(stamp(0)).unwrap();
        let latest = worker.request_runtime_snapshot(stamp(256)).unwrap();
        assert!(latest > first);
        assert!(
            worker
                .request_runtime_snapshot(Stamp {
                    generation: 12,
                    ..stamp(0)
                })
                .is_err()
        );
        thread::sleep(Duration::from_millis(10));
        assert!(
            worker.poll_runtime_snapshot().is_none(),
            "inspection never advances the native clock"
        );
        worker
            .realtime()
            .try_submit(Request::new(stamp(0), &[note(0)]).unwrap())
            .unwrap();
        let output = receive(&mut worker, stamp(0));
        assert!(
            output.audio.iter().all(|frame| *frame == [0.; 2]),
            "fixture executes nodes while remaining silent"
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let reply = loop {
            if let Some(reply) = worker.poll_runtime_snapshot() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(POLL);
        };
        assert_eq!(reply.request, latest);
        assert_eq!(reply.stamp.frame, 256);
        let snapshot = reply.snapshot.unwrap();
        assert!(
            Arc::ptr_eq(
                &snapshot,
                worker
                    .shared
                    .details
                    .lock()
                    .unwrap()
                    .runtime_report
                    .as_ref()
                    .unwrap()
            ),
            "reply and cached report share one graph allocation"
        );
        let evidence = &snapshot["evidence"];
        assert_eq!(evidence["frame"], 256);
        assert_eq!(evidence["audibility_verified"], false);
        assert_eq!(evidence["falcon_numerical_fidelity_verified"], false);
        let nodes = evidence["nodes"].as_array().unwrap();
        let oscillator = nodes
            .iter()
            .find(|node| node["kind"] == "MinBlepGenerator")
            .unwrap();
        assert!(oscillator["processed_blocks"].as_u64().unwrap() > 0);
        assert!(oscillator["retained_voice_instances"].as_u64().unwrap() > 0);
        let gains: Vec<_> = nodes.iter().filter(|node| node["kind"] == "Gain").collect();
        assert_eq!(gains[0]["currently_bypassed"], true);
        assert_eq!(gains[0]["processed_blocks"], 0);
        assert_eq!(gains[1]["currently_bypassed"], false);
        assert!(gains[1]["processed_blocks"].as_u64().unwrap() > 0);
        let script = nodes
            .iter()
            .find(|node| node["kind"] == "ScriptProcessor")
            .unwrap();
        assert!(script["processed_blocks"].is_null());
        assert_eq!(script["evidence_source"], "not_instrumented");
        worker
            .realtime()
            .try_submit(Request::new(stamp(256), &[]).unwrap())
            .unwrap();
        receive(&mut worker, stamp(256));
        assert_eq!(
            worker.diagnostic_report()["runtime_snapshot"]["stamp"]["frame"],
            256,
            "cached snapshots preserve their actual capture boundary"
        );
        assert_eq!(worker.stats().processed_frame, 512);
        worker.stop();
        assert!(worker.request_runtime_snapshot(stamp(512)).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn explicit_runtime_diagnostic_helper_preserves_stamped_success_and_unavailable_outcomes() {
        let (config, _) =
            authored_bank_with_script("function onSave()error('inspection is read only')end");
        let path = config.bank.clone();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(3)).unwrap();
        let captured = worker.runtime_diagnostic_report(Duration::from_millis(500));
        assert_eq!(captured["runtime_snapshot_request"]["outcome"], "captured");
        assert_eq!(captured["runtime_snapshot"]["stamp"]["frame"], 0);
        assert_eq!(
            captured["stats"]["rendered_blocks"], 0,
            "inspection cannot advance playback"
        );
        let latest = worker.shared.details.lock().unwrap().runtime_latest;
        let skipped = worker.runtime_diagnostic_report(Duration::ZERO);
        assert_eq!(skipped["runtime_snapshot_request"]["outcome"], "timeout");
        assert_eq!(
            worker.shared.details.lock().unwrap().runtime_latest,
            latest,
            "an exhausted shared export budget does not enqueue another graph inspection"
        );
        worker.stop();
        let stopped = worker.runtime_diagnostic_report(Duration::from_millis(500));
        assert_eq!(
            stopped["runtime_snapshot_request"]["outcome"],
            "unavailable"
        );
        assert_eq!(stopped["status"], "stopped");
        assert_eq!(
            stopped["runtime_snapshot"]["stamp"]["frame"], 0,
            "unavailable inspection preserves the earlier explicit capture stamp"
        );
        std::fs::remove_file(path).unwrap();
    }

    fn stamp(frame: u64) -> Stamp {
        Stamp {
            epoch: 7,
            generation: 11,
            frame,
        }
    }
    fn note(frame: u64) -> Input {
        Input {
            frame,
            kind: InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            },
        }
    }
    fn output(stamp: Stamp) -> Output {
        Output {
            stamp,
            audio: [[0.; 2]; BLOCK_FRAMES],
            commands: 0,
            host_commands: 0,
            logs: 0,
            dropped_logs: 0,
            rejected_ui: 0,
        }
    }

    #[test]
    fn fixed_packets_reject_bad_inputs_preserve_backpressure_and_future_output() {
        assert!(!std::mem::needs_drop::<Request>());
        assert!(!std::mem::needs_drop::<Output>());
        assert!(!std::mem::needs_drop::<Realtime<'_>>());
        let shared = Shared::new(7, 11);
        let mut cursor = PacketCursor::default();
        let mut realtime = Realtime {
            shared: &shared,
            cursor: Some(&mut cursor),
        };
        let check = || {
            let mut packet = Request::new(stamp(0), &[note(0)]).unwrap();
            packet.stamp.epoch = 6;
            assert_eq!(
                realtime.try_submit(packet).unwrap_err().reason,
                PacketError::WrongEpoch
            );
            packet.stamp = Stamp {
                generation: 10,
                ..stamp(0)
            };
            assert_eq!(
                realtime.try_submit(packet).unwrap_err().reason,
                PacketError::WrongGeneration
            );
            packet.stamp = stamp(256);
            assert_eq!(
                realtime.try_submit(packet).unwrap_err().reason,
                PacketError::WrongFrame
            );
            packet.stamp = stamp(0);
            packet.inputs[0] = Input {
                frame: 0,
                kind: InputKind::Transport {
                    playing: true,
                    beat: 0.,
                    tempo: 1e100,
                },
            };
            assert_eq!(
                realtime.try_submit(packet).unwrap_err().reason,
                PacketError::InvalidInput
            );
            packet.input_count = 257;
            assert_eq!(
                realtime.try_submit(packet).unwrap_err().reason,
                PacketError::TooManyInputs
            );
            assert!(Request::new(stamp(0), &[note(2), note(1)]).is_err());
            assert!(Request::new(stamp(0), &[note(256)]).is_err());
            for index in 0..QUEUE_CAPACITY {
                let frame = index as u64 * BLOCK_FRAMES as u64;
                realtime
                    .try_submit(Request::new(stamp(frame), &[note(frame)]).unwrap())
                    .unwrap();
            }
            let frame = QUEUE_CAPACITY as u64 * BLOCK_FRAMES as u64;
            let rejected = realtime
                .try_submit(Request::new(stamp(frame), &[note(frame)]).unwrap())
                .unwrap_err();
            assert_eq!(rejected.reason, PacketError::Full);
            assert_eq!(rejected.request.stamp, stamp(frame));
            assert_eq!(rejected.request.inputs[0].frame, frame);
            for index in 0..QUEUE_CAPACITY {
                assert_eq!(
                    shared.requests.pop().unwrap().stamp,
                    stamp(index as u64 * 256)
                );
            }
            realtime.try_submit(rejected.request).unwrap();
            assert_eq!(shared.requests.pop().unwrap().stamp, stamp(frame));
            assert_eq!(
                realtime.try_receive(stamp(0)).unwrap_err(),
                PacketError::Underrun
            );
            shared
                .outputs
                .push(output(Stamp {
                    epoch: 6,
                    ..stamp(0)
                }))
                .unwrap();
            shared.outputs.push(output(stamp(512))).unwrap();
            assert_eq!(
                realtime.try_receive(stamp(0)).unwrap_err(),
                PacketError::Underrun
            );
            assert_eq!(
                realtime.try_receive(stamp(256)).unwrap_err(),
                PacketError::Underrun
            );
            assert_eq!(realtime.try_receive(stamp(512)).unwrap().stamp, stamp(512));
            assert_eq!(
                realtime.try_receive(stamp(256)).unwrap_err(),
                PacketError::WrongFrame
            );
            shared.outputs.push(output(stamp(256))).unwrap();
            shared.outputs.push(output(stamp(768))).unwrap();
            assert_eq!(realtime.try_receive(stamp(768)).unwrap().stamp, stamp(768));
            shared.stop.store(true, Ordering::Release);
            assert_eq!(
                realtime
                    .try_submit(Request::new(stamp(frame + 256), &[]).unwrap())
                    .unwrap_err()
                    .reason,
                PacketError::Stopped
            );
            assert_eq!(
                realtime.try_receive(stamp(1024)).unwrap_err(),
                PacketError::Stopped
            );
        };
        assert_eq!(
            crate::test_support::allocations(check),
            0,
            "packet paths allocate or free"
        );
        let stats = shared.stats();
        assert_eq!(stats.backpressure, 1);
        assert_eq!(stats.underruns, 3);
        assert_eq!(stats.stale_packets, 2);
        assert_eq!(stats.errors, 6);
    }

    // Original synthetic UFS metadata and Program, using the existing authored
    // UFS record contract. No bank/sample/library content is embedded or copied.
    fn authored_bank() -> (StartConfig, String) {
        authored_bank_with_script(
            r#"
            local count=0
            function onInit()
                knob=Knob{name='authored',value=0.25}
                knob.changed=function()error('snapshot ran callback')end
            end
            function onNote(e)count=count+1;postEvent(e);wait(8);changeVolume(e.id,0.25,false,true)end
            function onController(e)assert(count==1);Program:setParameter('Gain',0.8)end
            function onRelease(e)postEvent(e)end
        "#,
        )
    }

    pub(crate) fn authored_bank_with_script(script: &str) -> (StartConfig, String) {
        let source = format!(
            r#"<Program Gain="0.5"><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" StartPhase="0.25" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
        );
        authored_bank_with_program(&source)
    }

    fn authored_bank_with_program(source: &str) -> (StartConfig, String) {
        fn append(bytes: &mut Vec<u8>, payload: &[u8]) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
            bytes.extend_from_slice(payload);
            pointer
        }
        fn named(bytes: &mut Vec<u8>, tag: u32, name: &str, length: usize, key: u64) -> u64 {
            let mut payload = vec![0; length];
            payload[..4].copy_from_slice(&tag.to_le_bytes());
            payload[4..4 + name.len()].copy_from_slice(name.as_bytes());
            crypto::transform(&mut payload[4..260], key, bytes.len() as u64 + 12);
            append(bytes, &payload)
        }
        let namespace = b"authored worker metadata";
        let key = crypto::metadata_key(namespace, "WorkerAuthored");
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..62].copy_from_slice(b"WorkerAuthored");
        let root = named(&mut bytes, 0x2fba_3632, "Root", 272, key);
        let member = named(&mut bytes, 0x6758_50e4, "authored.uvip", 289, key);
        let data = append(&mut bytes, source.as_bytes());
        bytes[member as usize + 260..member as usize + 268]
            .copy_from_slice(&(source.len() as u64).to_le_bytes());
        bytes[member as usize + 268..member as usize + 276].copy_from_slice(&data.to_le_bytes());
        let mut descriptor = vec![0; 34];
        descriptor[..4].copy_from_slice(&0x1847_b398u32.to_le_bytes());
        let descriptor = append(&mut bytes, &descriptor);
        bytes[root as usize + 260..root as usize + 268].copy_from_slice(&descriptor.to_le_bytes());
        let pointer = bytes.len() as u64 + 8;
        let mut table = vec![0; 16932];
        table[..4].copy_from_slice(&0x3ca8_6aafu32.to_le_bytes());
        table[4..8].copy_from_slice(&1u32.to_le_bytes());
        table[8..21].copy_from_slice(b"authored.uvip");
        crypto::transform(&mut table[8..264], key, pointer + 8);
        table[264..272].copy_from_slice(&member.to_le_bytes());
        table[272..288].fill(255);
        append(&mut bytes, &table);
        for field in [4, 12, 20] {
            bytes[descriptor as usize + field..descriptor as usize + field + 8]
                .copy_from_slice(&pointer.to_le_bytes());
        }
        let length = bytes.len() as u64;
        bytes[32..40].copy_from_slice(&length.to_le_bytes());
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let bank =
            std::env::temp_dir().join(format!("kontra-worker-{}-{unique}.ufs", std::process::id()));
        std::fs::write(&bank, bytes).unwrap();
        (
            StartConfig {
                bank,
                expected_bank_uuid: None,
                member: "authored.uvip".into(),
                metadata_namespace: namespace.to_vec(),
                program_namespace: b"authored worker program".to_vec(),
                content_key: None,
                content_bank: Some("WorkerAuthored".into()),
                sample_rate: 48000,
            },
            source.to_owned(),
        )
    }

    fn receive(worker: &mut Worker, expected: Stamp) -> Output {
        let started = Instant::now();
        loop {
            match worker.realtime().try_receive(expected) {
                Ok(output) => return output,
                Err(PacketError::Underrun) => {
                    assert!(started.elapsed() < Duration::from_secs(5));
                    thread::sleep(POLL);
                }
                Err(error) => panic!("{error:?}: {:?}", worker.private_failure()),
            }
        }
    }

    fn receive_ui(worker: &Worker, request: u64) -> UiSnapshotReply {
        let started = Instant::now();
        loop {
            if let Some(reply) = worker.poll_ui_snapshot() {
                assert_eq!(reply.request, request, "superseded UI reply escaped");
                return reply;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(POLL);
        }
    }

    #[test]
    fn extracted_audio_port_is_exclusive_and_preserves_queued_packet_cursors() {
        fn assert_send<T: Send>() {}
        assert_send::<AudioPort>();
        let shared = Arc::new(Shared::new(7, 11));
        let mut worker = Worker {
            shared: Arc::clone(&shared),
            thread: None,
            cursor: Some(PacketCursor::default()),
        };
        worker
            .realtime()
            .try_submit(Request::new(stamp(0), &[]).unwrap())
            .unwrap();
        shared.outputs.push(output(stamp(512))).unwrap();
        assert_eq!(
            worker.realtime().try_receive(stamp(0)).unwrap_err(),
            PacketError::Underrun
        );
        let mut port = worker.take_audio_port().unwrap();
        assert!(worker.take_audio_port().is_none());
        let check = || {
            let packet = Request::new(stamp(256), &[]).unwrap();
            assert_eq!(
                worker.realtime().try_submit(packet).unwrap_err().reason,
                PacketError::PortTaken
            );
            assert_eq!(
                worker.realtime().try_receive(stamp(512)).unwrap_err(),
                PacketError::PortTaken
            );
            port.realtime().try_submit(packet).unwrap();
            assert_eq!(shared.requests.pop().unwrap().stamp, stamp(0));
            assert_eq!(shared.requests.pop().unwrap().stamp, stamp(256));
            assert_eq!(
                port.realtime().try_receive(stamp(256)).unwrap_err(),
                PacketError::Underrun
            );
            assert_eq!(
                port.realtime().try_receive(stamp(512)).unwrap().stamp,
                stamp(512)
            );
            shared.status.store(Status::Failed as u8, Ordering::Release);
            assert_eq!(
                port.realtime().try_receive(stamp(768)).unwrap_err(),
                PacketError::Failed
            );
            assert_eq!(
                port.realtime()
                    .try_submit(Request::new(stamp(512), &[]).unwrap())
                    .unwrap_err()
                    .reason,
                PacketError::Failed
            );
            shared.stop.store(true, Ordering::Release);
            assert_eq!(
                port.realtime().try_receive(stamp(768)).unwrap_err(),
                PacketError::Stopped
            );
        };
        assert_eq!(crate::test_support::allocations(check), 0);
        drop(port);
        worker.stop();
    }

    #[test]
    fn extracted_audio_port_runs_while_controller_services_owned_ui_snapshots() {
        let (config, source) = authored_bank();
        let path = config.bank.clone();
        let program = parse_program(&source).unwrap();
        let processor = program
            .nodes
            .iter()
            .position(|node| node.kind == "ScriptProcessor")
            .unwrap();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(5)).unwrap();
        let mut port = worker.take_audio_port().unwrap();
        port.realtime()
            .try_submit(Request::new(stamp(0), &[note(0)]).unwrap())
            .unwrap();
        let start = Instant::now();
        let output = loop {
            match port.realtime().try_receive(stamp(0)) {
                Ok(output) => break output,
                Err(PacketError::Underrun) => {
                    assert!(start.elapsed() < Duration::from_secs(5));
                    thread::sleep(POLL);
                }
                Err(error) => panic!("{error:?}"),
            }
        };
        assert_eq!(output.stamp, stamp(0));
        assert!(
            output.commands > 0
                && output
                    .audio
                    .iter()
                    .flatten()
                    .all(|sample| sample.is_finite())
        );
        assert!(
            output
                .audio
                .iter()
                .flatten()
                .any(|sample| sample.abs() > 1e-5)
        );
        let request = worker.request_ui_snapshot(processor).unwrap();
        let snapshot = receive_ui(&worker, request);
        assert_eq!(snapshot.stamp, stamp(256));
        assert!(snapshot.snapshot.is_ok());
        drop(port); // Serialized retirement precedes controller stop/join.
        worker.stop();
        assert_eq!(worker.status(), Status::Stopped);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn worker_ui_snapshots_coalesce_are_owned_and_do_not_advance_idle_playback() {
        let (config, source) = authored_bank();
        let path = config.bank.clone();
        let program = parse_program(&source).unwrap();
        let processor = program
            .nodes
            .iter()
            .position(|node| node.kind == "ScriptProcessor")
            .unwrap();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.request_ui_snapshot(usize::MAX).unwrap();
        let latest = worker.request_ui_snapshot(processor).unwrap();
        worker.wait_ready(Duration::from_secs(5)).unwrap();
        let reply = receive_ui(&worker, latest);
        assert_eq!(reply.stamp, stamp(0));
        assert_eq!(reply.processor, processor);
        let snapshot = reply.snapshot.ok().unwrap();
        assert!(matches!(
            snapshot.widgets[0].value,
            Some(super::super::host::UiValue::Number(0.25))
        ));
        assert_eq!(worker.stats().rendered_blocks, 0);
        assert!(worker.poll_ui_snapshot().is_none());
        let invalid = worker.request_ui_snapshot(usize::MAX).unwrap();
        let reply = receive_ui(&worker, invalid);
        assert_eq!(reply.stamp, stamp(0));
        assert!(matches!(reply.snapshot, Err(UiSnapshotError::Unavailable)));
        assert_eq!(worker.status(), Status::Ready);
        assert_eq!(worker.stats().errors, 0);
        worker.stop();
        assert_eq!(
            worker.request_ui_snapshot(processor),
            Err(PacketError::Stopped)
        );
        drop(worker);
        assert!(snapshot.widgets[0].name == "authored");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn dedicated_worker_matches_persistent_player_and_stops_with_both_queues_full() {
        let (config, source) = authored_bank();
        let path = config.bank.clone();
        let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
        let loaded = library
            .program(&config.member, &config.program_namespace)
            .unwrap();
        assert_eq!(
            loaded.program.nodes.len(),
            parse_program(&source).unwrap().nodes.len()
        );
        let resources = BankResources::new(library, &loaded.path, Default::default()).unwrap();
        let mut expected = Player::new(
            &loaded.program,
            BTreeMap::new(),
            resources,
            config.sample_rate,
        )
        .unwrap();
        let inputs = [
            note(0),
            Input {
                frame: 300,
                kind: InputKind::Controller {
                    channel: 0,
                    controller: 1,
                    value: 127,
                },
            },
        ];
        let baseline = expected.render(&inputs, 512).unwrap();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(5)).unwrap();
        worker
            .realtime()
            .try_submit(Request::new(stamp(0), &inputs[..1]).unwrap())
            .unwrap();
        worker
            .realtime()
            .try_submit(Request::new(stamp(256), &inputs[1..]).unwrap())
            .unwrap();
        let first = receive(&mut worker, stamp(0));
        let second = receive(&mut worker, stamp(256));
        assert_eq!(
            [first.audio.as_slice(), second.audio.as_slice()].concat(),
            baseline.audio
        );
        assert!(first.audio.iter().any(|frame| frame[0] != 0.));
        assert_eq!(first.commands + second.commands, baseline.commands as u32);
        assert_eq!(
            first.host_commands + second.host_commands,
            baseline.host_commands as u32
        );
        assert_eq!(
            second.commands, 1,
            "Lua callback must resume across packet boundary"
        );
        let stats = worker.stats();
        assert_eq!(stats.rendered_blocks, 2);
        assert!(
            stats.initialization_ns > 0
                && stats.render_ns >= stats.max_render_ns
                && stats.max_render_ns > 0
        );
        assert_eq!(stats.errors, 0);
        // Fill eight queued outputs plus one rendered packet waiting to publish.
        for index in 2..11 {
            let mut packet = Request::new(stamp(index * 256), &[]).unwrap();
            loop {
                match worker.realtime().try_submit(packet) {
                    Ok(()) => break,
                    Err(Rejected {
                        reason: PacketError::Full,
                        request,
                    }) => {
                        packet = request;
                        thread::sleep(POLL);
                    }
                    Err(error) => panic!("{error:?}"),
                }
            }
        }
        let started = Instant::now();
        while worker.stats().rendered_blocks < 11 {
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(POLL);
        }
        assert_eq!(worker.shared.outputs.len(), QUEUE_CAPACITY);
        // The UI mailbox must remain responsive while audio output is full.
        let processor = loaded
            .program
            .nodes
            .iter()
            .position(|node| node.kind == "ScriptProcessor")
            .unwrap();
        let request = worker.request_ui_snapshot(processor).unwrap();
        let reply = receive_ui(&worker, request);
        assert_eq!(reply.stamp, stamp(11 * 256));
        assert!(reply.snapshot.is_ok());
        assert_eq!(worker.stats().rendered_blocks, 11);
        for index in 11..19 {
            worker
                .realtime()
                .try_submit(Request::new(stamp(index * 256), &[note(index * 256)]).unwrap())
                .unwrap();
        }
        assert_eq!(worker.shared.requests.len(), QUEUE_CAPACITY);
        worker.stop();
        assert_eq!(worker.status(), Status::Stopped);
        let stats = worker.stats();
        assert_eq!(stats.rendered_blocks, 11);
        assert_eq!(stats.cancelled_requests, 8);
        assert_eq!(stats.cancelled_outputs, 9);
        assert!(worker.private_failure().is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn initialization_failure_is_private_and_fixed_errors_cross_the_port() {
        let (mut config, _) = authored_bank();
        let path = config.bank.clone();
        config.content_bank = Some("wrong authored bank".into());
        let mut worker = Worker::start(config, 7, 11).unwrap();
        assert!(worker.wait_ready(Duration::from_secs(5)).is_err());
        assert_eq!(worker.status(), Status::Failed);
        assert!(worker.private_failure().unwrap().contains("different bank"));
        let rejected = worker
            .realtime()
            .try_submit(Request::new(stamp(0), &[note(0)]).unwrap())
            .unwrap_err();
        assert_eq!(rejected.reason, PacketError::Failed);
        assert_eq!(rejected.request.stamp, stamp(0));
        assert_eq!(
            worker.realtime().try_receive(stamp(0)).unwrap_err(),
            PacketError::Failed
        );
        worker.stop();
        assert_eq!(worker.shared.requests.len(), 0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn worker_rechecks_catalog_bank_uuid_before_becoming_ready() {
        let (mut config, _) = authored_bank();
        let path = config.bank.clone();
        config.expected_bank_uuid = Some([1; 16]);
        let mut worker = Worker::start(config, 7, 11).unwrap();
        assert!(worker.wait_ready(Duration::from_secs(5)).is_err());
        assert_eq!(worker.status(), Status::Failed);
        assert!(
            worker
                .private_failure()
                .unwrap()
                .contains("bank identity changed")
        );
        assert_eq!(worker.stats().rendered_blocks, 0);
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }

    fn ui_input(frame: u64, processor: NodeId, value: super::super::host::UiEditValue) -> UiInput {
        UiInput {
            frame,
            edit: super::super::host::UiEdit {
                processor,
                widget: 1,
                value,
                modifiers: Default::default(),
            },
        }
    }

    #[test]
    fn fixed_ui_packets_revalidate_public_fields_without_callback_allocations() {
        use super::super::host::UiEditValue;
        let shared = Shared::new(7, 11);
        let mut cursor = PacketCursor::default();
        let mut realtime = Realtime {
            shared: &shared,
            cursor: Some(&mut cursor),
        };
        let check = || {
            let edit = ui_input(0, 2, UiEditValue::Number(0.75));
            let packet = Request::new_with_ui(stamp(0), &[], &[edit]).unwrap();
            let mut bad = packet;
            bad.ui_count = (MAX_UI_EDITS + 1) as u16;
            assert_eq!(
                realtime.try_submit(bad).unwrap_err().reason,
                PacketError::TooManyInputs
            );
            bad = packet;
            bad.ui_inputs[0].edit.value = UiEditValue::Number(f64::NAN);
            assert_eq!(
                realtime.try_submit(bad).unwrap_err().reason,
                PacketError::InvalidInput
            );
            bad = packet;
            bad.ui_inputs[0].frame = 256;
            assert_eq!(
                realtime.try_submit(bad).unwrap_err().reason,
                PacketError::InvalidInput
            );
            assert!(Request::new_with_ui(stamp(0), &[], &[edit; MAX_UI_EDITS + 1]).is_err());
            assert!(
                Request::new_with_ui(stamp(0), &[], &[UiInput { frame: 1, ..edit }, edit]).is_err()
            );
            realtime.try_submit(packet).unwrap();
            let queued = shared.requests.pop().unwrap();
            assert_eq!(queued.ui_count, 1);
            assert!(queued.ui_inputs[0].edit == edit.edit);
        };
        assert_eq!(crate::test_support::allocations(check), 0);
    }

    #[test]
    fn worker_ui_edits_share_midi_clock_yield_across_packets_and_preserve_pcm() {
        use super::super::host::{UiEditValue, UiValue};
        let (config, source) = authored_bank_with_script(
            r#"
            function onInit()
                knob=Knob{name='authored',value=0.25}
                knob.changed=function(self)
                    Program:setParameter('Gain',self.value)
                    wait(8)
                    Program:setParameter('Gain',self.value*0.5)
                end
            end
            function onNote(e)assert(knob.value==0.75);postEvent(e)end
            function onController(e)assert(knob.value==0.5)end
        "#,
        );
        let path = config.bank.clone();
        let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
        let loaded = library
            .program(&config.member, &config.program_namespace)
            .unwrap();
        let processor = parse_program(&source)
            .unwrap()
            .nodes
            .iter()
            .position(|node| node.kind == "ScriptProcessor")
            .unwrap();
        let resources = BankResources::new(library, &loaded.path, Default::default()).unwrap();
        let mut expected = Player::new(&loaded.program, BTreeMap::new(), resources, 48000).unwrap();
        let inputs = [
            note(0),
            Input {
                frame: 300,
                kind: InputKind::Controller {
                    channel: 0,
                    controller: 1,
                    value: 127,
                },
            },
        ];
        let edits = [
            ui_input(0, processor, UiEditValue::Number(0.75)),
            UiInput {
                frame: 123,
                edit: super::super::host::UiEdit {
                    widget: 99,
                    ..ui_input(0, processor, UiEditValue::Number(0.1)).edit
                },
            },
            ui_input(180, processor, UiEditValue::Boolean(true)),
            ui_input(300, processor, UiEditValue::Number(0.5)),
        ];
        let malformed = ui_input(0, processor, UiEditValue::Number(f64::NAN));
        assert!(expected.render_with_ui(&inputs, &[malformed], 768).is_err());
        assert_eq!(expected.current_frame(), 0);
        assert!(matches!(
            expected.ui_snapshot(processor).unwrap().widgets[0].value,
            Some(UiValue::Number(0.25))
        ));
        let baseline = expected.render_with_ui(&inputs, &edits, 768).unwrap();
        assert_eq!(baseline.rejected_ui, 0b110);
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(5)).unwrap();
        worker
            .realtime()
            .try_submit(Request::new_with_ui(stamp(0), &inputs[..1], &edits[..3]).unwrap())
            .unwrap();
        worker
            .realtime()
            .try_submit(Request::new_with_ui(stamp(256), &inputs[1..], &edits[3..]).unwrap())
            .unwrap();
        worker
            .realtime()
            .try_submit(Request::new(stamp(512), &[]).unwrap())
            .unwrap();
        let first = receive(&mut worker, stamp(0));
        let second = receive(&mut worker, stamp(256));
        let third = receive(&mut worker, stamp(512));
        assert_eq!(
            [
                first.audio.as_slice(),
                second.audio.as_slice(),
                third.audio.as_slice()
            ]
            .concat(),
            baseline.audio
        );
        assert_eq!(first.rejected_ui, 0b110);
        assert_eq!(second.rejected_ui | third.rejected_ui, 0);
        assert_eq!(
            first.commands + second.commands + third.commands,
            baseline.commands as u32
        );
        assert_eq!(
            first.host_commands + second.host_commands + third.host_commands,
            baseline.host_commands as u32
        );
        let request = worker.request_ui_snapshot(processor).unwrap();
        let reply = receive_ui(&worker, request);
        assert_eq!(reply.stamp, stamp(768));
        assert!(reply.snapshot.ok().unwrap() == expected.ui_snapshot(processor).unwrap());
        assert_eq!(worker.stats().errors, 0);
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn ui_callback_failure_requires_player_replacement_and_fails_worker() {
        let (config, source) = authored_bank();
        let path = config.bank.clone();
        let library = Rc::new(Library::open(&path, &config.metadata_namespace, None).unwrap());
        let loaded = library
            .program(&config.member, &config.program_namespace)
            .unwrap();
        let processor = parse_program(&source)
            .unwrap()
            .nodes
            .iter()
            .position(|node| node.kind == "ScriptProcessor")
            .unwrap();
        let resources = BankResources::new(library, &loaded.path, Default::default()).unwrap();
        let mut player = Player::new(&loaded.program, BTreeMap::new(), resources, 48000).unwrap();
        let edit = ui_input(0, processor, super::super::host::UiEditValue::Number(0.75));
        assert!(player.render_with_ui(&[], &[edit], 256).is_err());
        assert!(player.render(&[], 256).is_err());
        assert!(player.ui_snapshot(processor).is_err());
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(Duration::from_secs(5)).unwrap();
        worker
            .realtime()
            .try_submit(Request::new_with_ui(stamp(0), &[], &[edit]).unwrap())
            .unwrap();
        let start = Instant::now();
        while worker.status() != Status::Failed {
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(POLL);
        }
        assert_eq!(worker.stats().rendered_blocks, 0);
        assert_eq!(
            worker.realtime().try_receive(stamp(0)).unwrap_err(),
            PacketError::Failed
        );
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }
}
#[cfg(test)]
#[path = "worker_hosted_tests.rs"]
mod worker_hosted_tests;
