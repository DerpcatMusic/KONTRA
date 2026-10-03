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
                HostedInput::Off { root, frame } => (Some(root), frame),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSnapshotError {
    Unavailable,
}

#[derive(Clone, Copy)]
struct UiSnapshotRequest {
    id: u64,
    processor: NodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Starting,
    Ready,
    Failed,
    Stopped,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    /// Rejected submissions (excluding Full) and fatal worker failures.
    pub errors: u64,
    /// Unsuccessful packet reads, not a hardware/audio-driver counter.
    pub underruns: u64,
    pub backpressure: u64,
    pub stale_packets: u64,
    pub rendered_blocks: u64,
    /// Retained renderer voice instances after its latest completed packet;
    /// includes held/releasing silent voices, not an audibility estimate.
    pub active_voices: u64,
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
    underruns: AtomicU64,
    backpressure: AtomicU64,
    stale_packets: AtomicU64,
    rendered_blocks: AtomicU64,
    active_voices: AtomicU64,
    initialization_ns: AtomicU64,
    render_ns: AtomicU64,
    max_render_ns: AtomicU64,
    render_deadline_misses: AtomicU64,
    cancelled_requests: AtomicU64,
    cancelled_outputs: AtomicU64,
    cancelled_completions: AtomicU64,
}

#[derive(Default)]
struct Details {
    failure: Option<String>,
    diagnostics: Vec<&'static str>,
    ui_processors: Vec<NodeId>,
    ui_latest: u64,
    ui_request: Option<UiSnapshotRequest>,
    ui_reply: Option<UiSnapshotReply>,
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
            details: Mutex::new(Details::default()),
            ui_pending: AtomicBool::new(false),
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
        Stats {
            errors: c.errors.load(Ordering::Relaxed),
            underruns: c.underruns.load(Ordering::Relaxed),
            backpressure: c.backpressure.load(Ordering::Relaxed),
            stale_packets: c.stale_packets.load(Ordering::Relaxed),
            rendered_blocks: c.rendered_blocks.load(Ordering::Relaxed),
            active_voices: c.active_voices.load(Ordering::Relaxed),
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
        Self::start_inner(config, epoch, generation, false)
    }
    pub fn start_hosted(config: StartConfig, epoch: u64, generation: u64) -> Result<Self> {
        Self::start_inner(config, epoch, generation, true)
    }
    fn start_inner(config: StartConfig, epoch: u64, generation: u64, hosted: bool) -> Result<Self> {
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
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run(config, &worker_shared, initialized)
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
    pub fn diagnostics(&self) -> Vec<&'static str> {
        self.shared
            .details
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .diagnostics
            .clone()
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
        self.shared
            .counters
            .active_voices
            .store(0, Ordering::Relaxed);
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
    /// Underrun never substitutes old audio or silently changes the input cursor.
    pub fn try_receive(&mut self, expected: Stamp) -> std::result::Result<Output, PacketError> {
        if let Some(output) = self.try_receive_available(expected)? {
            return Ok(output);
        }
        self.shared
            .counters
            .underruns
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

/// Controller observes an activation abort, not successful ends for discarded
/// roots. A future adapter must retire canonical owners through its lifecycle.
fn finish(shared: &Shared, failure: Option<String>) {
    shared.counters.active_voices.store(0, Ordering::Relaxed);
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

fn run(config: StartConfig, shared: &Shared, initialized: Instant) -> Result<()> {
    // Everything containing Rc, borrowed graph nodes, Lua or file authority is
    // created, used and destroyed in this stack frame on this dedicated thread.
    if shared.stop.load(Ordering::Acquire) {
        return Ok(());
    }
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
    let loaded = library.program(&config.member, &config.program_namespace)?;
    let unsupported = super::playback::preflight(&loaded.program);
    ensure!(
        unsupported.is_empty(),
        "Native UVI graph preflight failed: {}",
        serde_json::to_string(&unsupported)?
    );
    let resources = BankResources::new(library.clone(), &loaded.path, library.samples(&loaded)?)?;
    let mut player = if shared.hosted.is_some() {
        Player::new_hosted(
            &loaded.program,
            library.modules()?,
            resources,
            config.sample_rate,
            shared.stamp.epoch,
            shared.stamp.generation,
        )?
    } else {
        Player::new(
            &loaded.program,
            library.modules()?,
            resources,
            config.sample_rate,
        )?
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
    shared.status.store(Status::Ready as u8, Ordering::Release);
    serve(&mut player, shared, config.sample_rate)
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
        let rendered = rendered?;
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
        #[cfg(feature = "plugin")]
        assert_eq!(
            crate::plugin::tests::allocations(check),
            0,
            "packet paths allocate or free"
        );
        #[cfg(not(feature = "plugin"))]
        {
            let mut check = check;
            check();
        }
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
        let source = format!(
            r#"<Program Gain="0.5"><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" StartPhase="0.25" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
        );
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
            source,
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
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(check), 0);
        #[cfg(not(feature = "plugin"))]
        {
            let mut check = check;
            check();
        }
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
        #[cfg(feature = "plugin")]
        assert_eq!(crate::plugin::tests::allocations(check), 0);
        #[cfg(not(feature = "plugin"))]
        {
            let mut check = check;
            check();
        }
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
