#![doc = include_str!("../README.md")]

#[cfg(not(target_has_atomic = "64"))]
compile_error!("derpcat-flight-recorder requires lock-free 64-bit atomics");

mod journal;

use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_LABEL_BYTES: usize = 48;
const MAX_DETAIL_BYTES: usize = 384;
const MAX_FILE_BYTES: usize = 96;
const MAX_CODE_BYTES: usize = 48;
const MAX_USER_LINE_BYTES: usize = 160;
const WORKER_POLL: Duration = Duration::from_millis(10);
const PERIODIC_SYNC: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    Dialog,
    DragDrop,
    Export,
    Host,
    Playback,
    Realtime,
    Ui,
    #[default]
    Legacy,
    #[serde(other)]
    Other,
}

impl Subsystem {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dialog => "dialog",
            Self::DragDrop => "drag_drop",
            Self::Export => "export",
            Self::Host => "host",
            Self::Playback => "playback",
            Self::Realtime => "realtime",
            Self::Ui => "ui",
            Self::Legacy => "legacy",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Started,
    Progress,
    Completed,
    Failed,
    Cancelled,
    #[default]
    Event,
    #[serde(other)]
    Other,
}

impl Phase {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Progress => "progress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Event => "event",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Importance {
    #[default]
    Evidence,
    Sampled,
    Noise,
}

impl Importance {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Evidence => "evidence",
            Self::Sampled => "sampled",
            Self::Noise => "noise",
        }
    }

    #[must_use]
    pub const fn is_evidence(self) -> bool {
        matches!(self, Self::Evidence)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadRole {
    Gui,
    RecorderWriter,
    CrashReporter,
    Export,
    Audio,
    /// A named BUFFR worker thread: capture, storage, paging or the wall-clock drain. These are
    /// not the GUI, and calling them GUI is what sent the first pass of the #226/#227/#233
    /// investigation down the wrong path — every system-audio event in those logs read `[gui]`.
    Background,
    #[default]
    HostOrUnknown,
}

impl ThreadRole {
    #[must_use]
    pub fn from_thread_name(name: Option<&str>) -> Self {
        match name {
            Some("audio") => Self::Audio,
            Some("derpcat-flight-recorder") => Self::RecorderWriter,
            Some("buffr-crash-reporter" | "buffr-support-report") => Self::CrashReporter,
            Some("main") => Self::Gui,
            Some(name) if name.starts_with("buffr-drag") || name.contains("drag-render") => {
                Self::Export
            }
            // The editor runs on a thread the host owns and does not name `main`, so a
            // BUFFR-named thread is never the GUI.
            Some(name) if name.starts_with("buffr-") => Self::Background,
            _ => Self::HostOrUnknown,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::RecorderWriter => "recorder",
            Self::CrashReporter => "crash_reporter",
            Self::Export => "export",
            Self::Audio => "audio",
            Self::Background => "background",
            Self::HostOrUnknown => "host_or_unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub correlation_id: Option<u64>,
    pub instance_id: Option<u64>,
}

pub struct Event {
    sequence: u64,
    at: u64,
    elapsed_ms: u64,
    thread: String,
    subsystem: Subsystem,
    action: String,
    phase: Phase,
    context: Context,
    detail: String,
    importance: Importance,
    code: String,
    user_line: String,
    file: String,
    line: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Record {
    pub sequence: u64,
    pub at: u64,
    pub elapsed_ms: u64,
    pub subsystem: Subsystem,
    pub action: String,
    pub phase: Phase,
    pub correlation_id: Option<u64>,
    pub instance_id: Option<u64>,
    pub thread: String,
    pub detail: String,
    pub realtime_frame: Option<u64>,
    pub value_a: i64,
    pub value_b: i64,
    #[serde(skip_serializing_if = "is_default_importance")]
    pub importance: Importance,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user_line: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub line: u32,
}

fn is_default_importance(value: &Importance) -> bool {
    *value == Importance::Evidence
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

impl Record {
    #[must_use]
    pub fn is_evidence(&self) -> bool {
        self.importance.is_evidence() && self.action != "diagnostic_overflow"
    }

    #[must_use]
    pub fn fingerprint_action(&self) -> String {
        format!(
            "{}.{}.{}",
            self.subsystem.label(),
            self.action,
            self.phase.label()
        )
    }

    #[must_use]
    pub fn render_diagnostic(&self) -> String {
        let correlation = self
            .correlation_id
            .map(|id| format!(" correlation={id}"))
            .unwrap_or_default();
        let instance = self
            .instance_id
            .map(|id| format!(" instance={id}"))
            .unwrap_or_default();
        let realtime = self
            .realtime_frame
            .map(|frame| {
                format!(
                    " frame={frame} value_a={} value_b={}",
                    self.value_a, self.value_b
                )
            })
            .unwrap_or_default();
        let code = if self.code.is_empty() {
            String::new()
        } else {
            format!(" code={}", self.code)
        };
        let location = if self.file.is_empty() {
            String::new()
        } else {
            format!(" loc={}:{}", self.file, self.line)
        };
        let detail = if self.detail.is_empty() {
            String::new()
        } else {
            format!(" · {}", self.detail)
        };
        format!(
            "#{:05} {} +{}ms [{}] {}.{} {}{}{}{}{}{}{}",
            self.sequence,
            self.at,
            self.elapsed_ms,
            self.thread,
            self.subsystem.label(),
            self.action,
            self.phase.label(),
            correlation,
            instance,
            realtime,
            code,
            location,
            detail,
        )
    }

    #[must_use]
    pub fn render(&self) -> String {
        self.render_diagnostic()
    }
}

pub struct Emit<'a> {
    pub subsystem: Subsystem,
    pub action: &'a str,
    pub phase: Phase,
    pub context: Context,
    pub detail: &'a str,
    pub importance: Importance,
    pub code: &'a str,
    pub user_line: &'a str,
    pub location: Option<&'static std::panic::Location<'static>>,
    pub thread_role: Option<ThreadRole>,
}

#[derive(Clone)]
pub struct Sink {
    event_sender: mpsc::SyncSender<Event>,
    control_sender: mpsc::Sender<Control>,
    sequence: Arc<AtomicU64>,
    correlation: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    started: Instant,
}

impl Sink {
    #[track_caller]
    pub fn record(
        &self,
        subsystem: Subsystem,
        action: impl Into<String>,
        phase: Phase,
        context: Context,
        detail: impl Into<String>,
    ) {
        let action = action.into();
        let detail = detail.into();
        self.emit(Emit {
            subsystem,
            action: &action,
            phase,
            context,
            detail: &detail,
            importance: Importance::Evidence,
            code: "",
            user_line: "",
            location: Some(std::panic::Location::caller()),
            thread_role: None,
        });
    }

    #[track_caller]
    pub fn emit(&self, emit: Emit<'_>) {
        if matches!(emit.importance, Importance::Noise | Importance::Sampled) {
            return;
        }
        let location = match emit.location {
            Some(location) => location,
            None => std::panic::Location::caller(),
        };
        let event = self.make_event(&emit, location);
        if self.event_sender.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[track_caller]
    pub fn record_durable(
        &self,
        subsystem: Subsystem,
        action: impl Into<String>,
        phase: Phase,
        context: Context,
        detail: impl Into<String>,
        timeout: Duration,
    ) -> bool {
        let action = action.into();
        let detail = detail.into();
        let event = self.make_event(
            &Emit {
                subsystem,
                action: &action,
                phase,
                context,
                detail: &detail,
                importance: Importance::Evidence,
                code: "",
                user_line: "",
                location: None,
                thread_role: None,
            },
            std::panic::Location::caller(),
        );
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        self.control_sender
            .send(Control::Durable(event, acknowledge))
            .is_ok()
            && acknowledged
                .recv_timeout(timeout)
                .is_ok_and(|success| success)
    }

    #[track_caller]
    pub fn begin_action(
        &self,
        subsystem: Subsystem,
        action: impl Into<String>,
        mut context: Context,
        detail: impl Into<String>,
    ) -> Action {
        if context.correlation_id.is_none() {
            context.correlation_id = Some(self.correlation.fetch_add(1, Ordering::Relaxed));
        }
        let action = bounded(action.into(), MAX_LABEL_BYTES);
        let _ = self.record_durable(
            subsystem,
            action.clone(),
            Phase::Started,
            context,
            detail,
            Duration::from_millis(250),
        );
        Action {
            sink: self.clone(),
            subsystem,
            action,
            context,
        }
    }

    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn next_correlation_id(&self) -> u64 {
        self.correlation.fetch_add(1, Ordering::Relaxed)
    }

    fn make_event(&self, emit: &Emit<'_>, location: &std::panic::Location<'static>) -> Event {
        Event {
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            at: now_unix(),
            elapsed_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            thread: emit
                .thread_role
                .unwrap_or_else(|| ThreadRole::from_thread_name(std::thread::current().name()))
                .label()
                .to_string(),
            subsystem: emit.subsystem,
            action: bounded(emit.action.to_string(), MAX_LABEL_BYTES),
            phase: emit.phase,
            context: emit.context,
            detail: bounded(emit.detail.to_string(), MAX_DETAIL_BYTES),
            importance: emit.importance,
            code: bounded(emit.code.to_string(), MAX_CODE_BYTES),
            user_line: bounded(emit.user_line.to_string(), MAX_USER_LINE_BYTES),
            file: bounded_source_file(location.file()),
            line: location.line(),
        }
    }
}

#[must_use]
pub struct Action {
    sink: Sink,
    subsystem: Subsystem,
    action: String,
    context: Context,
}

impl Action {
    #[track_caller]
    pub fn completed(self, detail: impl Into<String>) {
        self.finish(Phase::Completed, detail);
    }

    #[track_caller]
    pub fn failed(self, detail: impl Into<String>) {
        self.finish(Phase::Failed, detail);
    }

    #[track_caller]
    pub fn cancelled(self, detail: impl Into<String>) {
        self.finish(Phase::Cancelled, detail);
    }

    #[track_caller]
    fn finish(self, phase: Phase, detail: impl Into<String>) {
        self.sink
            .record(self.subsystem, self.action, phase, self.context, detail);
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct RtEvent {
    pub event_code: u16,
    pub phase: u8,
    pub flags: u8,
    pub frame_clock: u64,
    pub value_a: i64,
    pub value_b: i64,
}

pub mod rt_code {
    pub const PROCESS_STARTED: u16 = 1;
    pub const BUFFER_SHAPE_CHANGED: u16 = 2;
    pub const DEADLINE_MISSED: u16 = 3;
    pub const NON_FINITE_AUDIO: u16 = 4;
    pub const BUFFER_OVERRUN: u16 = 5;
}

pub struct RtWriter {
    producer: Producer<RtEnvelope>,
    sequence: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    instance_id: u64,
}

impl RtWriter {
    #[inline]
    pub fn try_record(&mut self, event: RtEvent) {
        let envelope = RtEnvelope {
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            instance_id: self.instance_id,
            event,
        };
        if self.producer.push(envelope).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Clone, Copy)]
struct RtEnvelope {
    sequence: u64,
    instance_id: u64,
    event: RtEvent,
}

pub struct Recorder {
    sink: Sink,
    control_sender: mpsc::Sender<Control>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl Recorder {
    pub fn start(
        bootstrap_path: impl Into<PathBuf>,
        event_capacity: usize,
        journal_bytes: usize,
    ) -> std::io::Result<Self> {
        let bootstrap_path = bootstrap_path.into();
        let journal = journal::Journal::create(&bootstrap_path, journal_bytes)?;
        let (event_sender, event_receiver) = mpsc::sync_channel(event_capacity.max(1));
        let (control_sender, control_receiver) = mpsc::channel();
        let sequence = Arc::new(AtomicU64::new(1));
        let dropped = Arc::new(AtomicU64::new(0));
        let sink = Sink {
            event_sender,
            control_sender: control_sender.clone(),
            sequence: Arc::clone(&sequence),
            correlation: Arc::new(AtomicU64::new(1)),
            dropped: Arc::clone(&dropped),
            started: Instant::now(),
        };
        let worker_sequence = Arc::clone(&sequence);
        let worker_dropped = Arc::clone(&dropped);
        let worker = std::thread::Builder::new()
            .name("derpcat-flight-recorder".to_string())
            .spawn(move || {
                writer_worker(
                    journal,
                    journal_bytes,
                    event_receiver,
                    control_receiver,
                    worker_sequence,
                    worker_dropped,
                );
            })?;
        Ok(Self {
            sink,
            control_sender,
            worker: Mutex::new(Some(worker)),
        })
    }

    #[must_use]
    pub fn sink(&self) -> Sink {
        self.sink.clone()
    }

    pub fn register_rt_lane(&self, instance_id: u64, capacity: usize) -> RtWriter {
        let (producer, consumer) = RingBuffer::new(capacity.max(1));
        let _ = self.control_sender.send(Control::AddRt(consumer));
        RtWriter {
            producer,
            sequence: Arc::clone(&self.sink.sequence),
            dropped: Arc::clone(&self.sink.dropped),
            instance_id,
        }
    }

    pub fn rotate(&self, path: impl Into<PathBuf>, timeout: Duration) -> std::io::Result<()> {
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        self.control_sender
            .send(Control::Rotate(path.into(), acknowledge))
            .map_err(|_| std::io::Error::other("flight recorder worker stopped"))?;
        acknowledged
            .recv_timeout(timeout)
            .map_err(|_| std::io::Error::other("flight recorder rotate timed out"))?
            .map_err(std::io::Error::other)
    }

    pub fn flush(&self, timeout: Duration) -> bool {
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        self.control_sender
            .send(Control::Flush(acknowledge))
            .is_ok()
            && acknowledged
                .recv_timeout(timeout)
                .is_ok_and(|success| success)
    }

    pub fn suspend(&self, timeout: Duration) -> bool {
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        self.control_sender
            .send(Control::Suspend(acknowledge))
            .is_ok()
            && acknowledged
                .recv_timeout(timeout)
                .is_ok_and(|success| success)
    }

    pub fn shutdown(&self, timeout: Duration) -> bool {
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        let persisted = self
            .control_sender
            .send(Control::Shutdown(acknowledge))
            .is_ok()
            && acknowledged
                .recv_timeout(timeout)
                .is_ok_and(|success| success);
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        let joined = worker.map_or(true, |worker| worker.join().is_ok());
        persisted && joined
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.shutdown(Duration::from_millis(250));
    }
}

enum Control {
    Durable(Event, mpsc::SyncSender<bool>),
    AddRt(Consumer<RtEnvelope>),
    Rotate(PathBuf, mpsc::SyncSender<Result<(), String>>),
    Flush(mpsc::SyncSender<bool>),
    Suspend(mpsc::SyncSender<bool>),
    Shutdown(mpsc::SyncSender<bool>),
}

fn writer_worker(
    initial_journal: journal::Journal,
    journal_bytes: usize,
    event_receiver: mpsc::Receiver<Event>,
    control_receiver: mpsc::Receiver<Control>,
    sequence: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
) {
    let mut journal = Some(initial_journal);
    let started = Instant::now();
    let mut rt_consumers = Vec::<Consumer<RtEnvelope>>::new();
    let mut dirty = false;
    let mut last_sync = Instant::now();
    let mut persisted_drops = 0;
    let mut io_ok = true;
    loop {
        while let Ok(control) = control_receiver.try_recv() {
            match control {
                Control::Durable(event, acknowledge) => {
                    let persisted = journal.as_mut().is_some_and(|journal| {
                        let written = journal.write(&event.into_record()).is_ok();
                        let synced = journal.sync().is_ok();
                        written && synced
                    });
                    io_ok &= persisted;
                    dirty = false;
                    last_sync = Instant::now();
                    let _ = acknowledge.send(io_ok);
                }
                Control::AddRt(consumer) => rt_consumers.push(consumer),
                Control::Rotate(path, acknowledge) => {
                    if let Some(journal) = journal.as_mut() {
                        let _ = journal.sync();
                    }
                    let result = journal::Journal::create(&path, journal_bytes)
                        .map(|new_journal| journal = Some(new_journal))
                        .map_err(|error| error.to_string());
                    if result.is_ok() {
                        dropped.store(0, Ordering::Relaxed);
                        persisted_drops = 0;
                        io_ok = true;
                    }
                    dirty = false;
                    last_sync = Instant::now();
                    let _ = acknowledge.send(result);
                }
                Control::Flush(acknowledge) => {
                    io_ok &= drain_events(&event_receiver, &mut journal, &mut dirty);
                    io_ok &= drain_rt(&mut rt_consumers, &mut journal, started, &mut dirty);
                    if let Some(journal) = journal.as_mut() {
                        io_ok &= journal.sync().is_ok();
                    }
                    dirty = false;
                    last_sync = Instant::now();
                    let _ = acknowledge.send(io_ok);
                }
                Control::Suspend(acknowledge) => {
                    io_ok &= drain_events(&event_receiver, &mut journal, &mut dirty);
                    io_ok &= drain_rt(&mut rt_consumers, &mut journal, started, &mut dirty);
                    if let Some(journal) = journal.as_mut() {
                        io_ok &= journal.sync().is_ok();
                    }
                    journal = None;
                    dirty = false;
                    last_sync = Instant::now();
                    let _ = acknowledge.send(io_ok);
                }
                Control::Shutdown(acknowledge) => {
                    io_ok &= drain_events(&event_receiver, &mut journal, &mut dirty);
                    io_ok &= drain_rt(&mut rt_consumers, &mut journal, started, &mut dirty);
                    if let Some(journal) = journal.as_mut() {
                        io_ok &= journal.sync().is_ok();
                    }
                    let _ = acknowledge.send(io_ok);
                    return;
                }
            }
        }

        io_ok &= drain_rt(&mut rt_consumers, &mut journal, started, &mut dirty);
        match event_receiver.recv_timeout(WORKER_POLL) {
            Ok(event) => {
                if let Some(journal) = journal.as_mut() {
                    io_ok &= journal.write(&event.into_record()).is_ok();
                    dirty = true;
                }
                io_ok &= drain_events(&event_receiver, &mut journal, &mut dirty);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if dirty && last_sync.elapsed() >= PERIODIC_SYNC {
            if let Some(journal) = journal.as_mut() {
                io_ok &=
                    persist_drop_count(journal, &sequence, &dropped, &mut persisted_drops, started);
                io_ok &= journal.sync().is_ok();
            }
            dirty = false;
            last_sync = Instant::now();
        }
    }
}

fn persist_drop_count(
    journal: &mut journal::Journal,
    sequence: &AtomicU64,
    dropped: &AtomicU64,
    persisted: &mut u64,
    started: Instant,
) -> bool {
    let current = dropped.load(Ordering::Relaxed);
    if current == *persisted {
        return true;
    }
    let record = Record {
        sequence: sequence.fetch_add(1, Ordering::Relaxed),
        at: now_unix(),
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        subsystem: Subsystem::Other,
        action: "diagnostic_overflow".to_string(),
        phase: Phase::Event,
        thread: ThreadRole::RecorderWriter.label().to_string(),
        detail: "bounded diagnostic events were dropped".to_string(),
        value_a: i64::try_from(current).unwrap_or(i64::MAX),
        importance: Importance::Sampled,
        code: "journal.overflow".to_string(),
        ..Record::default()
    };
    let written = journal.write(&record).is_ok();
    if written {
        *persisted = current;
    }
    written
}

fn drain_events(
    receiver: &mpsc::Receiver<Event>,
    journal: &mut Option<journal::Journal>,
    dirty: &mut bool,
) -> bool {
    let mut success = true;
    while let Ok(event) = receiver.try_recv() {
        if let Some(journal) = journal.as_mut() {
            success &= journal.write(&event.into_record()).is_ok();
            *dirty = true;
        }
    }
    success
}

fn drain_rt(
    consumers: &mut Vec<Consumer<RtEnvelope>>,
    journal: &mut Option<journal::Journal>,
    started: Instant,
    dirty: &mut bool,
) -> bool {
    let mut success = true;
    consumers.retain_mut(|consumer| {
        while let Ok(envelope) = consumer.pop() {
            let action = rt_action(envelope.event.event_code);
            let record = Record {
                sequence: envelope.sequence,
                at: now_unix(),
                elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                subsystem: Subsystem::Realtime,
                action: action.clone(),
                phase: phase_from_rt(envelope.event.phase),
                instance_id: Some(envelope.instance_id),
                thread: ThreadRole::Audio.label().to_string(),
                realtime_frame: Some(envelope.event.frame_clock),
                value_a: envelope.event.value_a,
                value_b: envelope.event.value_b,
                importance: Importance::Evidence,
                code: action,
                ..Record::default()
            };
            if let Some(journal) = journal.as_mut() {
                success &= journal.write(&record).is_ok();
                *dirty = true;
            }
        }
        !consumer.is_abandoned()
    });
    success
}

fn phase_from_rt(phase: u8) -> Phase {
    match phase {
        1 => Phase::Started,
        2 => Phase::Progress,
        3 => Phase::Completed,
        4 => Phase::Failed,
        5 => Phase::Cancelled,
        _ => Phase::Event,
    }
}

fn rt_action(code: u16) -> String {
    match code {
        rt_code::PROCESS_STARTED => "process_started".to_string(),
        rt_code::BUFFER_SHAPE_CHANGED => "buffer_shape_changed".to_string(),
        rt_code::DEADLINE_MISSED => "deadline_missed".to_string(),
        rt_code::NON_FINITE_AUDIO => "non_finite_audio".to_string(),
        rt_code::BUFFER_OVERRUN => "buffer_overrun".to_string(),
        _ => format!("event_{code}"),
    }
}

impl Event {
    fn into_record(self) -> Record {
        Record {
            sequence: self.sequence,
            at: self.at,
            elapsed_ms: self.elapsed_ms,
            subsystem: self.subsystem,
            action: self.action,
            phase: self.phase,
            correlation_id: self.context.correlation_id,
            instance_id: self.context.instance_id,
            thread: self.thread,
            detail: self.detail,
            importance: self.importance,
            code: self.code,
            user_line: self.user_line,
            file: self.file,
            line: self.line,
            ..Record::default()
        }
    }
}

impl fmt::Debug for Sink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Sink")
            .field("dropped_events", &self.dropped_events())
            .finish_non_exhaustive()
    }
}

#[must_use]
pub fn read_journal(path: impl AsRef<Path>, limit: usize) -> Vec<Record> {
    journal::read(path.as_ref(), limit).unwrap_or_default()
}

pub use journal::JournalCapture;

pub fn capture_journal(
    path: impl AsRef<Path>,
    head: usize,
    tail: usize,
    stopping: &std::sync::atomic::AtomicBool,
) -> std::io::Result<JournalCapture> {
    journal::capture(path.as_ref(), head, tail, stopping)
}

fn bounded(value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        value
    } else {
        let boundary = value
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= max_bytes)
            .last()
            .unwrap_or(0);
        value[..boundary].to_string()
    }
}

fn bounded_source_file(file: &str) -> String {
    let normalized = file.replace('\\', "/");
    let trimmed = if let Some((_, rest)) = normalized.rsplit_once("/src/") {
        format!("src/{rest}")
    } else if let Some(rest) = normalized.strip_prefix("src/") {
        format!("src/{rest}")
    } else if let Some((_, rest)) = normalized.rsplit_once("/vendor/") {
        format!("vendor/{rest}")
    } else if let Some(rest) = normalized.strip_prefix("vendor/") {
        format!("vendor/{rest}")
    } else {
        normalized
    };
    bounded(trimmed, MAX_FILE_BYTES)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_journal(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dfr-{}-{}-{name}", std::process::id(), now_unix()));
        let _ = fs::create_dir_all(&dir);
        dir.join("journal.dfr")
    }

    fn emit_here(sink: &Sink) {
        sink.record(
            Subsystem::Host,
            "plugin_initialize",
            Phase::Failed,
            Context::default(),
            "audio engine initialization returned false",
        );
    }

    fn oversized_event() -> Event {
        Event {
            sequence: 1,
            at: now_unix(),
            elapsed_ms: 0,
            thread: ThreadRole::HostOrUnknown.label().to_string(),
            subsystem: Subsystem::Host,
            action: "oversized".to_string(),
            phase: Phase::Event,
            context: Context::default(),
            detail: "x".repeat(2_048),
            importance: Importance::Evidence,
            code: String::new(),
            user_line: String::new(),
            file: String::new(),
            line: 0,
        }
    }

    #[test]
    fn flush_reports_journal_write_failure() {
        let path = temp_journal("flush-write-failure");
        let recorder = Recorder::start(&path, 32, 64 * 1024).expect("start recorder");
        recorder
            .sink
            .event_sender
            .send(oversized_event())
            .expect("queue oversized event");

        assert!(!recorder.flush(Duration::from_secs(1)));
        let _ = recorder.shutdown(Duration::from_secs(1));
        drop(recorder);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn suspend_reports_journal_write_failure() {
        let path = temp_journal("suspend-write-failure");
        let recorder = Recorder::start(&path, 32, 64 * 1024).expect("start recorder");
        recorder
            .sink
            .event_sender
            .send(oversized_event())
            .expect("queue oversized event");

        assert!(!recorder.suspend(Duration::from_secs(1)));
        let _ = recorder.shutdown(Duration::from_secs(1));
        drop(recorder);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn shutdown_reports_journal_write_failure() {
        let path = temp_journal("shutdown-write-failure");
        let recorder = Recorder::start(&path, 32, 64 * 1024).expect("start recorder");
        recorder
            .sink
            .event_sender
            .send(oversized_event())
            .expect("queue oversized event");

        assert!(!recorder.shutdown(Duration::from_secs(1)));
        drop(recorder);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn durable_record_reports_suspended_journal() {
        let path = temp_journal("durable-suspended");
        let recorder = Recorder::start(&path, 32, 64 * 1024).expect("start recorder");
        assert!(recorder.suspend(Duration::from_secs(1)));

        assert!(!recorder.sink().record_durable(
            Subsystem::DragDrop,
            "external_drag",
            Phase::Started,
            Context::default(),
            "queued",
            Duration::from_secs(1),
        ));

        let _ = recorder.shutdown(Duration::from_secs(1));
        drop(recorder);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn shutdown_joins_writer_after_acknowledgement_timeout() {
        let path = temp_journal("joined-shutdown");
        let recorder = Recorder::start(&path, 32, 64 * 1024).expect("start recorder");
        let (blocked_acknowledge, blocked) = mpsc::sync_channel(0);
        recorder
            .control_sender
            .send(Control::Durable(oversized_event(), blocked_acknowledge))
            .expect("queue blocking acknowledgement");
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(blocked);
        });

        let started = Instant::now();
        assert!(!recorder.shutdown(Duration::from_millis(20)));
        assert!(started.elapsed() >= Duration::from_millis(50));

        release.join().expect("release blocked writer");
        assert!(recorder
            .worker
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_none());
        drop(recorder);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn record_event_location_points_at_caller() {
        let path = temp_journal("location");
        let recorder = Recorder::start(&path, 32, 64 * 1024).expect("start recorder");
        emit_here(&recorder.sink());
        assert!(recorder.flush(Duration::from_secs(1)));
        assert!(recorder.shutdown(Duration::from_secs(1)));
        let records = read_journal(&path, 16);
        let _ = fs::remove_file(&path);
        let record = records
            .iter()
            .find(|record| record.action == "plugin_initialize")
            .expect("initialize record");
        assert!(record.file.contains("lib.rs"), "file was {}", record.file);
        assert!(record.line > 0);
        assert_eq!(record.thread, ThreadRole::HostOrUnknown.label());
    }

    #[test]
    fn noise_is_not_persisted_and_evidence_survives_sampled_flood() {
        let path = temp_journal("admission");
        let recorder = Recorder::start(&path, 512, 256 * 1024).expect("start recorder");
        let sink = recorder.sink();
        for _ in 0..256 {
            sink.emit(Emit {
                subsystem: Subsystem::Ui,
                action: "pointer",
                phase: Phase::Event,
                context: Context::default(),
                detail: "pressed Left",
                importance: Importance::Noise,
                code: "ui.pointer",
                user_line: "",
                location: None,
                thread_role: None,
            });
            sink.emit(Emit {
                subsystem: Subsystem::Ui,
                action: "activity",
                phase: Phase::Event,
                context: Context::default(),
                detail: "Spectrogram perf: lod=0",
                importance: Importance::Sampled,
                code: "ui.spectrogram_perf",
                user_line: "",
                location: None,
                thread_role: None,
            });
        }
        sink.record(
            Subsystem::Host,
            "plugin_initialize",
            Phase::Failed,
            Context::default(),
            "audio engine initialization returned false",
        );
        assert!(recorder.flush(Duration::from_secs(1)));
        assert!(recorder.shutdown(Duration::from_secs(1)));
        let records = read_journal(&path, 512);
        let _ = fs::remove_file(&path);
        assert!(records.iter().all(|record| {
            record.action != "pointer"
                && record.action != "activity"
                && record.importance == Importance::Evidence
        }));
        let evidence: Vec<_> = records
            .iter()
            .filter(|record| record.is_evidence())
            .collect();
        assert_eq!(
            evidence.last().map(|record| record.action.as_str()),
            Some("plugin_initialize")
        );
        assert_eq!(
            evidence.last().map(|record| record.phase),
            Some(Phase::Failed)
        );
    }

    #[test]
    fn source_file_keeps_src_prefix_on_unix_and_windows() {
        assert_eq!(
            bounded_source_file(r"C:\Users\x\proj\src\support\crash.rs"),
            "src/support/crash.rs"
        );
        assert_eq!(bounded_source_file("src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn old_records_without_new_fields_deserialize() {
        let json = r#"{"sequence":1,"at":1,"elapsed_ms":0,"subsystem":"host","action":"plugin_deactivate","phase":"completed","thread":"unnamed","detail":"workers stopped"}"#;
        let record: Record = serde_json::from_str(json).expect("compat");
        assert_eq!(record.importance, Importance::Evidence);
        assert!(record.file.is_empty());
        assert_eq!(record.line, 0);
        assert!(record.is_evidence());
    }
}
