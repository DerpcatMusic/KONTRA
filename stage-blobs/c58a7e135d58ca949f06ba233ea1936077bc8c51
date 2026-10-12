//! Structured diagnostics for loaders, inspection commands and the editor.
//! Never call this module from an audio callback: runtime faults must first be
//! drained through the existing bounded audio-to-loader snapshot channel.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const SCHEMA_VERSION: u32 = 1;
pub const HISTORY_LIMIT: usize = 2048;
const HISTORY_BYTES: usize = 2 * 1024 * 1024;
const EVENT_BYTES: usize = 32 * 1024;
const DETAIL_BYTES: usize = EVENT_BYTES / 4;
const LOG_LIMIT: u64 = 8 * 1024 * 1024;
const QUEUE_LIMIT: usize = 256;
static NEXT_LOAD: AtomicU64 = AtomicU64::new(1);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
static REVISION: AtomicU64 = AtomicU64::new(0);
static OWNERS: AtomicUsize = AtomicUsize::new(0);
static MANAGER: OnceLock<Mutex<Option<Arc<Session>>>> = OnceLock::new();
static EXPORTS: OnceLock<Mutex<HashMap<u64, ExportStatus>>> = OnceLock::new();
static NEXT_EXPORT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}
impl LogLevel {
    fn index(self) -> usize {
        match self {
            Self::Debug => 0,
            Self::Info => 1,
            Self::Warning => 2,
            Self::Error => 3,
        }
    }
}

/// Fields may be extended; consumers should ignore unknown fields.
/// `timestamp_ms` is Unix time in UTC; monotonic time is relative to this session.
/// JSON `data` retains the existing journal contract.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEvent {
    pub schema_version: u32,
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub monotonic_ms: u64,
    pub session_id: String,
    pub level: LogLevel,
    pub module: String,
    pub event: String,
    pub stage: Option<String>,
    pub code: Option<String>,
    pub load_id: Option<String>,
    pub path: Option<String>,
    pub library: Option<String>,
    pub instance_id: Option<u64>,
    pub script_epoch: Option<u64>,
    pub outcome: Option<String>,
    pub program: Option<u32>,
    pub part: Option<usize>,
    pub script_slot: Option<u32>,
    pub line: Option<u32>,
    pub reason: Option<String>,
    #[serde(rename = "data", alias = "details")]
    pub details: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LogStatus {
    pub log_path: Option<PathBuf>,
    pub total_events: u64,
    /// Debug, Info, Warning, Error, counting all session events.
    pub level_counts: [u64; 4],
    pub history_evicted: u64,
    pub dropped_events: u64,
    pub truncated_events: u64,
    pub write_errors: u64,
    /// Complete journal records written this session, including rotated history.
    #[serde(default)]
    pub journal_events_written: u64,
    pub last_error: Option<String>,
    pub retention_errors: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticSnapshot {
    pub revision: u64,
    pub events: Vec<LogEvent>,
    pub status: LogStatus,
    pub build: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ExportStatus {
    Running,
    Complete {
        path: PathBuf,
        partial: bool,
        warnings: Vec<String>,
    },
    Failed {
        error: String,
    },
}

struct History {
    events: VecDeque<(LogEvent, usize)>,
    bytes: usize,
    status: LogStatus,
}
enum Command {
    Event(LogEvent),
    #[cfg(test)]
    Pause(mpsc::Sender<()>, mpsc::Receiver<()>),
    #[cfg(feature = "plugin")]
    NativeTiming(moose::mui::window::NativeTimingReport, u64),
    Flush(mpsc::Sender<Result<(), String>>),
    Export {
        id: u64,
        path: PathBuf,
        context: Value,
        snapshot: DiagnosticSnapshot,
    },
    Stop,
}
struct Session {
    id: String,
    path: PathBuf,
    started: Instant,
    history: Arc<Mutex<History>>,
    sender: Mutex<Option<SyncSender<Command>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    export_stopping: Arc<AtomicBool>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn root() -> PathBuf {
    #[cfg(not(test))]
    {
        std::env::var_os("KONTRA_LOG_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::cache_dir().map(|p| p.join("kontra").join("logs")))
            .unwrap_or_else(|| std::env::temp_dir().join("kontra-logs"))
    }
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!("kontra-test-{}/logs", std::process::id()))
    }
}
fn session() -> Arc<Session> {
    let mut manager = lock(MANAGER.get_or_init(|| Mutex::new(None)));
    manager
        .get_or_insert_with(|| Session::start(root()))
        .clone()
}
impl Session {
    fn start(root: PathBuf) -> Arc<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = format!(
            "{}-{nanos}-{}",
            std::process::id(),
            NEXT_SESSION.fetch_add(1, Ordering::Relaxed)
        );
        let path = root.join(format!("session-{id}.jsonl"));
        let history = Arc::new(Mutex::new(History {
            events: VecDeque::new(),
            bytes: 0,
            status: LogStatus {
                log_path: Some(path.clone()),
                ..Default::default()
            },
        }));
        let (sender, receiver) = mpsc::sync_channel(QUEUE_LIMIT);
        let worker_history = history.clone();
        let worker_path = path.clone();
        #[cfg(feature = "plugin")]
        let worker_id = id.clone();
        let started = Instant::now();
        let export_stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = export_stopping.clone();
        // Creation, serialization, rotation and export all happen off UI/audio.
        let worker = std::thread::Builder::new()
            .name("kontra-diagnostics".into())
            .spawn(move || {
                let mut journal = Journal::new(worker_path, worker_history);
                while let Ok(command) = receiver.recv() {
                    match command {
                        #[cfg(test)]
                        Command::Pause(entered, resume) => {
                            let _ = entered.send(());
                            let _ = resume.recv();
                        }
                        Command::Event(event) => {
                            if let Err(error) = journal.write_event(&event) {
                                journal.error(error);
                            }
                        }
                        #[cfg(feature = "plugin")]
                        Command::NativeTiming(report, capture_id) => {
                            let data = native_timing_summary(&report, capture_id);
                            let (mut row, bytes, truncated) = prepare_event(&worker_id, started, json!({
                                "level":"info", "module":"ui", "event":"native_frame_timing",
                                "code":"native_frame_timing", "data":data,
                            }));
                            retain_event(&mut lock(&journal.history), &mut row, bytes, truncated);
                            if let Err(error) = journal.write_event(&row) { journal.error(error); }
                        }
                        Command::Flush(reply) => {
                            let result = journal.flush().map_err(|e| e.to_string())
                                .and_then(|_| crate::support::flush_journal(Duration::from_secs(2)));
                            let _ = reply.send(result);
                        }
                        Command::Export {
                            id,
                            path,
                            context,
                            snapshot,
                        } => {
                            let flush_error = journal.flush_file().err().map(|error| {
                                let message = error.to_string();
                                journal.error(error);
                                message
                            });
                            let mut snapshot = snapshot;
                            snapshot.status = lock(&journal.history).status.clone();
                            let result = export_bundle_with_cancel(
                                &path,
                                &context,
                                &snapshot,
                                &journal.path,
                                flush_error,
                                &worker_stopping,
                            );
                            let status = match result {
                                Ok(coverage) => ExportStatus::Complete {
                                    path,
                                    partial: coverage.partial,
                                    warnings: coverage.warnings,
                                },
                                Err(error) => ExportStatus::Failed {
                                    error: error.to_string(),
                                },
                            };
                            lock(EXPORTS.get_or_init(|| Mutex::new(HashMap::new())))
                                .insert(id, status);
                            REVISION.fetch_add(1, Ordering::Release);
                        }
                        Command::Stop => break,
                    }
                }
                if let Err(error) = journal.flush() {
                    journal.error(error);
                }
                journal.close();
            });
        let (sender, worker) = match worker {
            Ok(worker) => (Some(sender), Some(worker)),
            Err(error) => {
                let mut h = lock(&history);
                h.status.last_error = Some(format!("Could not start diagnostics worker: {error}"));
                h.status.write_errors += 1;
                (None, None)
            }
        };
        let session = Arc::new(Self {
            id,
            path,
            started,
            history,
            sender: Mutex::new(sender),
            worker: Mutex::new(worker),
            export_stopping,
        });
        emit_to(
            &session,
            json!({
                "level":"info", "module":"system", "event":"session_started", "code":"session_started",
                "data":{"build":build_identity(), "os":std::env::consts::OS, "arch":std::env::consts::ARCH,
                    "pid":std::process::id()},
            }),
        );
        session
    }
}

/// Hold one on the top-level plugin/standalone owner. Clones share the lease;
/// the final owner joins the disk worker before plugin code can be unloaded.
#[derive(Clone)]
pub struct DiagnosticLease {
    _owner: Arc<LeaseOwner>,
}
impl Default for DiagnosticLease {
    fn default() -> Self {
        acquire()
    }
}
struct LeaseOwner;
pub fn acquire() -> DiagnosticLease {
    let mut manager = lock(MANAGER.get_or_init(|| Mutex::new(None)));
    OWNERS.fetch_add(1, Ordering::AcqRel);
    manager.get_or_insert_with(|| Session::start(root()));
    DiagnosticLease {
        _owner: Arc::new(LeaseOwner),
    }
}
impl Drop for LeaseOwner {
    fn drop(&mut self) {
        let mut manager = lock(MANAGER.get_or_init(|| Mutex::new(None)));
        if OWNERS.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = stop(&mut manager);
        }
    }
}

pub fn revision() -> u64 {
    REVISION.load(Ordering::Acquire)
}
pub fn build_identity() -> Value {
    serde_json::to_value(crate::build_info::BUILD).unwrap_or(Value::Null)
}
/// No disk access. Clone this bounded history on the editor's worker thread.
pub fn snapshot() -> DiagnosticSnapshot {
    let session = session();
    let history = lock(&session.history);
    DiagnosticSnapshot {
        revision: revision(),
        events: history.events.iter().map(|(e, _)| e.clone()).collect(),
        status: history.status.clone(),
        build: build_identity(),
    }
}
pub fn log_path() -> Option<PathBuf> {
    Some(session().path.clone())
}

/// Off-audio instrumentation. Recognized context fields are promoted from
/// `details`; never pass sample bytes, ciphertext, access data or full source
/// payloads. Script context must use the bounded off-thread excerpt helper.
pub fn event(level: LogLevel, module: &str, code: &str, details: Value) {
    emit(json!({"level":level,"module":module,"event":code,"code":code,"data":details}));
}
/// Enabled only at editor construction. The completed fixed buffer moves once
/// to the joined diagnostics worker; native callbacks never summarize or format it.
#[cfg(feature = "plugin")]
pub(crate) fn native_timing_hook() -> Option<moose::mui::window::NativeTimingHook> {
    if std::env::var("KONTRA_NATIVE_UI_TIMING").ok().as_deref() != Some("1") { return None; }
    let lease = acquire();
    let session = session();
    let sender = lock(&session.sender).as_ref()?.clone();
    let history = session.history.clone();
    static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(1);
    Some(Arc::new(move |report| {
        let _keep_worker_alive = &lease;
        let capture_id = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
        if sender.try_send(Command::NativeTiming(report, capture_id)).is_err() {
            let mut history = lock(&history);
            history.status.dropped_events += 1;
            history.status.last_error = Some("Native UI timing report dropped: diagnostics worker queue unavailable".into());
            REVISION.fetch_add(1, Ordering::Release);
        }
    }))
}

#[cfg(feature = "plugin")]
fn native_timing_summary(report: &moose::mui::window::NativeTimingReport, capture_id: u64) -> Value {
    use moose::mui::window::{NATIVE_METRICS, NATIVE_OUTCOMES, NATIVE_TIMING_LIMIT};
    let summary = report.summary();
    let metrics: serde_json::Map<_, _> = NATIVE_METRICS.iter().zip(summary.metrics).map(|(name, metric)| {
        ((*name).into(), json!({"count":metric.count, "mean_ns":metric.mean_ns,
            "p50_ns":metric.p50_ns, "p99_ns":metric.p99_ns, "max_ns":metric.max_ns}))
    }).collect();
    let outcomes: serde_json::Map<_, _> = NATIVE_OUTCOMES.iter().zip(summary.outcomes)
        .map(|(name, count)| ((*name).into(), json!(count))).collect();
    json!({"capture_id":capture_id, "build":build_identity(), "status":report.stop,
        "samples_retained":report.count, "sample_capacity":NATIVE_TIMING_LIMIT, "capture_duration_limit_ms":10000,
        "elapsed_ns":report.elapsed_ns, "capacity_stopped":report.stop == "capacity",
        "window_closed_early":report.stop == "window_closed", "pointer_moves":report.pointer_moves,
        "primary_drag_moves":report.drag_moves, "reentrant_callbacks":report.reentrant_callbacks,
        "new_scenes":summary.new_scenes, "primary_drag_callbacks":summary.dragging_callbacks,
        "physical_size":report.physical_size, "device_scale":report.device_scale, "geometry_changes":report.geometry_changes,
        "first_callback_offset_ns":report.samples.first().filter(|_|report.count > 0).map(|s|s.offset_ns),
        "last_callback_offset_ns":report.count.checked_sub(1).and_then(|i|report.samples.get(i)).map(|s|s.offset_ns),
        "metrics":metrics, "outcomes":outcomes,
        "measurement":"Native adapter callback wall time. Advance combines model polling, queued input, view builds and UI layout. Present combines CPU surface acquisition, render/upload/submit and presentation submission. Zero-duration or absent stages are excluded from stage statistics.",
        "limitations":"No expected cadence exposed; no display deadline counts. Submission is not compositor/display FPS or GPU completion. No forced GPU synchronization. Reentrant callbacks are counted separately and are not timing samples."})
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}
fn emit(value: Value) -> Option<String> {
    let session = session();
    emit_to(&session, value)
}
fn prepare_event(session_id: &str, started: Instant, value: Value) -> (LogEvent, usize, bool) {
    let data = value.get("data").cloned().unwrap_or_else(|| json!({}));
    let named = |key: &str| text(&value, key).or_else(|| text(&data, key));
    let number = |key: &str| value[key].as_u64().or_else(|| data[key].as_u64());
    let event_name = named("event").unwrap_or_else(|| "diagnostic".into());
    let level = serde_json::from_value(value["level"].clone()).unwrap_or_else(|_| {
        if matches!(
            event_name.as_str(),
            "runtime_issue" | "resource_issue" | "load_incomplete"
        ) {
            LogLevel::Warning
        } else if event_name == "issue" {
            if matches!(
                data["code"].as_str(),
                Some("failed" | "initialization_failed")
            ) {
                LogLevel::Error
            } else {
                LogLevel::Warning
            }
        } else if event_name == "load_finished" && data["status"] == "failed" {
            LogLevel::Error
        } else {
            LogLevel::Info
        }
    });
    let mut row = LogEvent {
        schema_version: SCHEMA_VERSION,
        sequence: 0,
        timestamp_ms: now(),
        monotonic_ms: started.elapsed().as_millis() as u64,
        session_id: session_id.to_owned(),
        level,
        module: named("module").unwrap_or_else(|| {
            if event_name == "runtime_issue" {
                "ksp"
            } else {
                "loader"
            }
            .into()
        }),
        event: event_name,
        stage: named("stage"),
        code: named("code"),
        load_id: named("load_id"),
        path: named("path"),
        library: named("library"),
        instance_id: number("instance_id"),
        script_epoch: number("script_epoch"),
        outcome: named("status"),
        program: number("program").and_then(|n| n.try_into().ok()),
        part: number("part").and_then(|n| n.try_into().ok()),
        script_slot: number("script_slot")
            .or_else(|| number("slot"))
            .and_then(|n| n.try_into().ok()),
        line: number("line").and_then(|n| n.try_into().ok()),
        reason: named("reason").or_else(|| named("message")),
        details: data,
    };
    if row.module == "ksp"
        && let Some(message) = row.reason.as_deref()
    {
        let (slot, line) = script_location(message);
        row.script_slot = row.script_slot.or(slot);
        row.line = row.line.or(line);
    }
    let mut truncated = false;
    for field in [&mut row.module, &mut row.event] {
        truncated |= field.len() > 128;
        truncate(field, 128);
    }
    for (field, maximum) in [
        (&mut row.stage, 128),
        (&mut row.code, 128),
        (&mut row.load_id, 128),
        (&mut row.path, 4096),
        (&mut row.library, 1024),
        (&mut row.reason, 4096),
        (&mut row.outcome, 128),
    ] {
        if let Some(field) = field {
            truncated |= field.len() > maximum;
            truncate(field, maximum);
        }
    }
    clean(&mut row.details, false);
    let mut bytes = serde_json::to_vec(&row)
        .map(|b| b.len())
        .unwrap_or(EVENT_BYTES + 1);
    if bytes > EVENT_BYTES {
        let mut summary = json!({"diagnostic_truncated":true,"original_bytes":bytes});
        for key in [
            "status",
            "code",
            "elapsed_ms",
            "stage",
            "message",
            "reason",
            "count",
        ] {
            if let Some(value) = row
                .details
                .get(key)
                .filter(|v| !v.is_object() && !v.is_array())
            {
                let mut value = value.clone();
                if let Value::String(value) = &mut value {
                    truncate(value, 1024);
                }
                summary[key] = value;
            }
        }
        row.details = summary;
        if let Some(reason) = &mut row.reason {
            truncate(reason, 4096);
        }
        bytes = serde_json::to_vec(&row)
            .map(|b| b.len())
            .unwrap_or(EVENT_BYTES);
        truncated = true;
    }
    (row, bytes, truncated)
}

fn retain_event(history: &mut History, row: &mut LogEvent, bytes: usize, truncated: bool) {
    if truncated {
        history.status.truncated_events += 1;
    }
    history.status.total_events += 1;
    history.status.level_counts[row.level.index()] += 1;
    row.sequence = REVISION.fetch_add(1, Ordering::AcqRel) + 1;
    history.bytes += bytes;
    history.events.push_back((row.clone(), bytes));
    while history.events.len() > HISTORY_LIMIT || history.bytes > HISTORY_BYTES {
        if let Some((_, bytes)) = history.events.pop_front() {
            history.bytes -= bytes;
            history.status.history_evicted += 1;
        }
    }
}

fn emit_to(session: &Session, value: Value) -> Option<String> {
    enqueue(session, value, false)
}

// LoadTrace runs on loader/catalog/picture workers or the CLI. Backpressure
// keeps their bursts on the existing bounded queue; UI/runtime events do not wait.
fn enqueue(session: &Session, value: Value, wait: bool) -> Option<String> {
    let (mut row, bytes, truncated) = prepare_event(&session.id, session.started, value);
    retain_event(&mut lock(&session.history), &mut row, bytes, truncated);
    let sender = lock(&session.sender).clone();
    // Neither mutex can remain held while waiting: the writer needs history
    // to report disk failures and successful delivery, and shutdown takes sender.
    let error = match sender {
        Some(sender) if wait => sender.send(Command::Event(row)).err()
            .map(|_| "Diagnostics worker is unavailable"),
        Some(sender) => sender.try_send(Command::Event(row)).err().map(|error| match error {
            TrySendError::Full(_) => "Diagnostics queue is full; event retained in recent history only",
            TrySendError::Disconnected(_) => "Diagnostics worker is unavailable",
        }),
        None => Some("Diagnostics worker is unavailable"),
    };
    let mut history = lock(&session.history);
    if let Some(error) = error {
        history.status.dropped_events += 1;
        Some(error.into())
    } else {
        history.status.last_error.clone()
    }
}

/// Bridge the existing off-thread KSP formatters to zero-based typed log fields.
/// The human-facing formatters and LiveFault.slot use one-based slot numbers.
/// Keep the original message, and prefer explicit JSON fields when supplied.
fn script_location(message: &str) -> (Option<u32>, Option<u32>) {
    fn leading_number(text: &str) -> Option<u32> {
        let end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        text[..end].parse().ok()
    }
    let Some(rest) = message
        .strip_prefix("Script ")
        .or_else(|| message.strip_prefix("Slot "))
    else {
        return (None, None);
    };
    let slot = leading_number(rest).and_then(|n| n.checked_sub(1));
    let line = rest
        .split_once("line ")
        .and_then(|(_, rest)| leading_number(rest));
    (slot, line)
}

/// Only resolved, cached plaintext script source belongs here, on a loader or
/// report worker. Never pass container bytes, ciphertext or access material.
/// Slots in this excerpt are human-facing (one-based), like LiveFault.slot.
pub(crate) fn script_excerpt(source: &str, slot: u32, line: u32, column: Option<u32>) -> Option<Value> {
    if line == 0 { return None; }
    #[derive(Serialize)]
    struct Excerpt {
        origin: &'static str,
        script_slot: u32,
        line: u32,
        column: Option<u32>,
        first_line: u32,
        last_line: u32,
        truncated: bool,
        text: String,
    }
    use std::fmt::Write;
    let mut excerpt = Excerpt {
        origin: "resolved instrument script (embedded or linked)",
        script_slot: slot, line, column, first_line: line.saturating_sub(2).max(1),
        last_line: 0, truncated: false, text: String::new(),
    };
    let mut found = false;
    for (at, source_line) in source.split('\n').enumerate() {
        let source_line = source_line.strip_suffix('\r').unwrap_or(source_line);
        let at = at as u32 + 1;
        if at > line.saturating_add(2) { break; }
        if at < excerpt.first_line { continue; }
        let first_column = if at == line { column.unwrap_or(1).saturating_sub(129) as usize } else { 0 };
        let mut text = String::new();
        let mut tail_clipped = false;
        for ch in source_line.chars().skip(first_column) {
            if text.len() + ch.len_utf8() > 512 {
                tail_clipped = true;
                break;
            }
            // Keep source line structure; terminal/control escapes are not code.
            text.push(if ch.is_control() && ch != '\t' { '?' } else { ch });
        }
        let clipped = tail_clipped || first_column != 0;
        excerpt.truncated |= clipped;
        let prefix = if first_column != 0 { "…" } else { "" };
        let suffix = if tail_clipped { "…" } else { "" };
        let _ = writeln!(excerpt.text, "{} {at:>6} | {prefix}{text}{suffix}", if at == line { ">" } else { " " });
        if at == line {
            found = true;
            if let Some(column) = column.filter(|&c| c > 0) {
                let offset = (column as usize - 1).saturating_sub(first_column);
                if offset <= text.chars().count() {
                    let before: String = text.chars().take(offset).map(|ch| if ch == '\t' { '\t' } else { ' ' }).collect();
                    let _ = writeln!(excerpt.text, "         | {}{before}^", if first_column != 0 { " " } else { "" });
                }
            }
        }
        excerpt.last_line = at;
    }
    found.then(|| serde_json::to_value(excerpt).unwrap())
}

/// Readable alongside issue maps or serialized events. LogEvent.details is
/// serialized as `data`; older/support callers may also use `details`.
pub(crate) fn excerpt_text(issue: &Value) -> Option<&str> {
    issue["source_excerpt"]["text"].as_str()
        .or_else(|| issue["data"]["source_excerpt"]["text"].as_str())
        .or_else(|| issue["details"]["source_excerpt"]["text"].as_str())
}

pub(crate) fn resource(instrument: &Path, name: &str, error: &str) {
    emit(
        json!({"module":"resources","event":"resource_issue","path":instrument,"data":{"code":"resource_unavailable","resource":name,"message":error}}),
    );
}

/// Off-UI barrier: all earlier accepted commands have reached the disk worker.
/// A timeout/write error is returned, never reported as a successful flush.
pub fn flush(timeout: Duration) -> Result<(), String> {
    let session = session();
    let (reply, receiver) = mpsc::channel();
    let sender = lock(&session.sender)
        .clone()
        .ok_or("Diagnostics worker is unavailable")?;
    sender
        .try_send(Command::Flush(reply))
        .map_err(|_| "Diagnostics queue is full or unavailable".to_owned())?;
    receiver
        .recv_timeout(timeout)
        .map_err(|e| format!("Diagnostics flush did not complete: {e}"))?
}
/// Call after all owners/loaders stop. Joining prevents worker code running
/// after DLL unload. Disk stalls can delay final owner destruction.
pub fn shutdown() -> Result<(), String> {
    let mut manager = lock(MANAGER.get_or_init(|| Mutex::new(None)));
    if OWNERS.load(Ordering::Acquire) != 0 {
        return Err("Diagnostic owners are still active".into());
    }
    stop(&mut manager)
}
fn stop(manager: &mut Option<Arc<Session>>) -> Result<(), String> {
    let Some(session) = manager.take() else {
        return Ok(());
    };
    session.export_stopping.store(true, Ordering::Release);
    let sender = lock(&session.sender).take();
    if let Some(sender) = sender {
        let _ = sender.send(Command::Stop);
    }
    if let Some(worker) = lock(&session.worker).take() {
        worker
            .join()
            .map_err(|_| "Diagnostics worker panicked".to_owned())?;
    }
    let history = lock(&session.history);
    if history.status.write_errors > 0 {
        Err(history
            .status
            .last_error
            .clone()
            .unwrap_or_else(|| "Diagnostic writes failed".into()))
    } else {
        Ok(())
    }
}

struct Journal {
    path: PathBuf,
    file: Option<File>,
    session_lock: Option<File>,
    bytes: u64,
    created: bool,
    needs_separator: bool,
    history: Arc<Mutex<History>>,
}
impl Journal {
    fn new(path: PathBuf, history: Arc<Mutex<History>>) -> Self {
        Self {
            path,
            file: None,
            session_lock: None,
            bytes: 0,
            created: false,
            needs_separator: false,
            history,
        }
    }
    fn open(&mut self) -> std::io::Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        let root = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("Log path has no parent"))?;
        std::fs::create_dir_all(root)?;
        if self.session_lock.is_none() {
            let guard = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(self.path.with_extension("lock"))?;
            guard.try_lock().map_err(std::io::Error::from)?;
            self.session_lock = Some(guard);
            if let Err(error) = retain_sessions(root, &self.path) {
                let mut history = lock(&self.history);
                history.status.retention_errors += 1;
                history.status.last_error = Some(format!("Log retention: {error}"));
                REVISION.fetch_add(1, Ordering::Release);
            }
        }
        // A session ID is unique across instances, processes and rapid restarts.
        let file = if self.created {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?
        } else {
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&self.path)?
        };
        self.bytes = file.metadata()?.len();
        self.created = true;
        self.file = Some(file);
        Ok(())
    }
    fn write_event(&mut self, event: &LogEvent) -> std::io::Result<()> {
        crate::support::journal_event(event);
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        if bytes.len() as u64 > LOG_LIMIT {
            return Err(std::io::Error::other(
                "Diagnostic event exceeds journal limit",
            ));
        }
        self.open()?;
        if self.bytes + bytes.len() as u64 + u64::from(self.needs_separator) > LOG_LIMIT {
            self.file.take();
            let previous = self.path.with_extension("previous.jsonl");
            match std::fs::remove_file(&previous) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            std::fs::rename(&self.path, &previous)?;
            self.created = false;
            self.needs_separator = false;
            self.open()?;
        }
        let result = (|| {
            let file = self.file.as_mut().unwrap();
            if self.needs_separator {
                file.write_all(b"\n")?;
                self.bytes += 1;
                self.needs_separator = false;
            }
            file.write_all(&bytes)
        })();
        if let Err(error) = result {
            self.file.take();
            self.bytes = std::fs::metadata(&self.path)
                .map(|m| m.len())
                .unwrap_or(self.bytes);
            self.needs_separator = true;
            return Err(error);
        }
        self.bytes += bytes.len() as u64;
        lock(&self.history).status.journal_events_written += 1;
        Ok(())
    }
    fn error(&self, error: std::io::Error) {
        let mut history = lock(&self.history);
        history.status.write_errors += 1;
        history.status.last_error = Some(error.to_string());
        REVISION.fetch_add(1, Ordering::Release);
    }
    fn flush_file(&mut self) -> std::io::Result<()> {
        if let Some(file) = &mut self.file {
            file.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if let Err(error) = self.flush_file() {
            self.error(std::io::Error::other(error.to_string()));
            return Err(error);
        }
        let history = lock(&self.history);
        if history.status.write_errors > 0 {
            Err(std::io::Error::other(
                history
                    .status
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "Diagnostic writes failed".into()),
            ))
        } else {
            Ok(())
        }
    }
    fn close(&mut self) {
        self.file.take();
        self.session_lock.take();
        if let Some(root) = self.path.parent() {
            if let Err(error) = retain_sessions(root, Path::new("")) {
                let mut history = lock(&self.history);
                history.status.retention_errors += 1;
                history.status.last_error = Some(format!("Log retention: {error}"));
                REVISION.fetch_add(1, Ordering::Release);
            }
        }
    }
}

/// Normalize rotations to their session base even if a crash happened between
/// renaming the primary and creating its replacement. Both callers use the
/// same catalog, lock name and metadata fallback for those orphan rotations.
fn session_catalog(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut sessions = BTreeSet::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with("session-")
            || !name.ends_with(".jsonl")
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        let base = name
            .strip_suffix(".previous.jsonl")
            .map(|name| root.join(format!("{name}.jsonl")))
            .unwrap_or(path);
        sessions.insert(base);
    }
    Ok(sessions.into_iter().collect())
}
fn session_metadata(path: &Path) -> std::io::Result<(SystemTime, u64)> {
    let primary = match std::fs::metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let previous = match std::fs::metadata(path.with_extension("previous.jsonl")) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let modified = primary
        .as_ref()
        .or(previous.as_ref())
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?
        .modified()?;
    Ok((
        modified,
        primary.map(|m| m.len()).unwrap_or(0) + previous.map(|m| m.len()).unwrap_or(0),
    ))
}
/// Crash logs become eligible as soon as the OS releases the session lock.
fn retain_sessions(root: &Path, current: &Path) -> std::io::Result<()> {
    let mut inactive = Vec::new();
    for path in session_catalog(root)? {
        if path == current {
            continue;
        }
        let guard = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        match guard.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => continue,
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let (modified, bytes) = match session_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let previous = path.with_extension("previous.jsonl");
        inactive.push((modified, path, previous, bytes, guard));
    }
    inactive.sort_by_key(|(modified, ..)| std::cmp::Reverse(*modified));
    let mut kept = 0_u64;
    for (modified, path, previous, bytes, _guard) in inactive {
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default();
        if age <= Duration::from_secs(7 * 24 * 60 * 60) && kept + bytes <= 64 * 1024 * 1024 {
            kept += bytes;
            continue;
        }
        for target in [&path, &previous] {
            match std::fs::remove_file(target) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        // Windows requires closing the lock handle before removing its file.
        drop(_guard);
        let _ = std::fs::remove_file(path.with_extension("lock"));
    }
    Ok(())
}

/// Preview contains only counts/policy/context, never scans journals on the UI.
pub fn export_preview(context: &Value) -> Value {
    let redact = context["redact_paths"].as_bool().unwrap_or(true);
    let mut context = context.clone();
    clean(&mut context, redact);
    let session = session();
    let history = lock(&session.history);
    let mut preview = json!({
        "schema_version": SCHEMA_VERSION, "build":build_identity(), "context":context,
        "redact_paths":redact, "status":history.status,
        "contents":["report.json","README.txt","events.jsonl","journal.jsonl (current and retained inactive sessions)", "crash-evidence/manifest.json", "crash-evidence/ (complete archived private crash originals and saved delivery records)"],
        "privacy":"Structured logs include bounded script excerpts and redact paths by default. Exact archived private crash originals and saved delivery records are also included UNREDACTED; they may contain personal paths or sensitive native fields. These local copies are never added to automatic uploads. This export does not search OS reports or copy binary process dumps. Review before sharing.",
        "history_limit":HISTORY_LIMIT,"history_bytes_limit":HISTORY_BYTES,
        "journal_limit_bytes":LOG_LIMIT,"rotations_retained":1,
        "inactive_retention_bytes":64*1024*1024,"inactive_retention_days":7,
    });
    clean(&mut preview, redact);
    preview
}

/// Enqueue export after earlier journal writes. `destination` must be a NEW
/// directory. Existing directories/files are never overwritten or removed.
/// `context.redact_paths` defaults to true; false includes local paths.
pub fn request_export(destination: PathBuf, mut context: Value) -> Result<u64, String> {
    if !context.is_object() {
        return Err("Report context must be a JSON object".into());
    }
    let redact = context["redact_paths"].as_bool().unwrap_or(true);
    clean(&mut context, redact);
    if serde_json::to_vec(&context)
        .map_err(|e| e.to_string())?
        .len()
        > 4 * 1024 * 1024
    {
        return Err("Report context exceeds the 4MiB limit; reduce included issue examples".into());
    }
    let session = session();
    let snapshot = snapshot();
    let id = NEXT_EXPORT.fetch_add(1, Ordering::Relaxed);
    let exports = EXPORTS.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let mut exports = lock(exports);
        if exports
            .values()
            .filter(|s| matches!(s, ExportStatus::Running))
            .count()
            >= 4
        {
            return Err("Four diagnostic exports are already queued".into());
        }
        // Finished job statuses are bounded; running jobs are never evicted.
        if exports.len() >= 32 {
            if let Some(oldest) = exports
                .iter()
                .filter(|(_, v)| !matches!(v, ExportStatus::Running))
                .map(|(id, _)| *id)
                .min()
            {
                exports.remove(&oldest);
            }
        }
        exports.insert(id, ExportStatus::Running);
    }
    let result = lock(&session.sender).as_ref().map(|sender| {
        sender.try_send(Command::Export {
            id,
            path: destination,
            context,
            snapshot,
        })
    });
    match result {
        Some(Ok(())) => {
            REVISION.fetch_add(1, Ordering::Release);
            Ok(id)
        }
        _ => {
            let error = "Diagnostic export queue is full or unavailable".to_owned();
            lock(exports).insert(
                id,
                ExportStatus::Failed {
                    error: error.clone(),
                },
            );
            Err(error)
        }
    }
}

pub fn export_status(id: u64) -> Option<ExportStatus> {
    EXPORTS
        .get()
        .and_then(|exports| lock(exports).get(&id).cloned())
}
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
struct ExportCoverage {
    partial: bool,
    warnings: Vec<String>,
}
/// Hold locks until copying completes so another process's retention cannot
/// delete crash history midway through this report. Other active sessions are
/// excluded; their owner can export its own journal without cross-instance IO.
fn export_journals(
    current: &Path,
    warnings: &mut Vec<String>,
) -> (Vec<(PathBuf, String)>, Vec<File>, u64) {
    let identity = |path: &Path| {
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown-session")
            .trim_start_matches("session-")
            .trim_end_matches(".jsonl")
            .to_owned()
    };
    let mut sessions = vec![(current.to_path_buf(), identity(current))];
    let mut guards = Vec::new();
    let mut active_omitted = 0;
    let root = current.parent().unwrap_or_else(|| Path::new(""));
    if let Err(error) = retain_sessions(root, current) {
        warnings.push(format!("Previous session retention failed: {error}"));
    }
    let entries = match session_catalog(root) {
        Ok(entries) => entries,
        Err(error) => {
            warnings.push(format!("Previous session catalog unavailable: {error}"));
            return (Vec::new(), guards, 0);
        }
    };
    for path in entries {
        if path == current {
            continue;
        }
        let guard = match OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
        {
            Ok(guard) => guard,
            Err(error) => {
                warnings.push(format!("Previous session lock unavailable: {error}"));
                continue;
            }
        };
        match guard.try_lock() {
            Ok(()) => {
                sessions.push((path.clone(), identity(&path)));
                guards.push(guard);
            }
            Err(std::fs::TryLockError::WouldBlock) => active_omitted += 1,
            Err(std::fs::TryLockError::Error(error)) => {
                warnings.push(format!("Previous session lock failed: {error}"))
            }
        }
    }
    sessions.sort_by_key(|(path, _)| {
        session_metadata(path)
            .map(|(modified, _)| modified)
            .unwrap_or(UNIX_EPOCH)
    });
    let mut paths = Vec::new();
    for (path, id) in sessions {
        paths.push((path.with_extension("previous.jsonl"), id.clone()));
        paths.push((path, id));
    }
    (paths, guards, active_omitted)
}
#[cfg(test)]
fn export_bundle(destination: &Path, context: &Value, snapshot: &DiagnosticSnapshot, journal: &Path, flush_error: Option<String>) -> std::io::Result<ExportCoverage> {
    export_bundle_with_cancel(destination, context, snapshot, journal, flush_error, &AtomicBool::new(false))
}
fn export_bundle_with_cancel(
    destination: &Path,
    context: &Value,
    snapshot: &DiagnosticSnapshot,
    journal: &Path,
    flush_error: Option<String>,
    stopping: &AtomicBool,
) -> std::io::Result<ExportCoverage> {
    let redact = context["redact_paths"].as_bool().unwrap_or(true);
    // create_dir provides the atomic no-overwrite boundary, including symlinks.
    std::fs::create_dir(destination)?;
    let result = (|| {
        let mut report = json!({
            "schema_version":SCHEMA_VERSION,"created_timestamp_ms":now(),"build":snapshot.build,
            "system":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},
            "context":context,"status":snapshot.status,"revision":snapshot.revision,
            "retained_events":snapshot.events.len(),"history_limit":HISTORY_LIMIT,
            "redact_paths":redact,
            "privacy":{"paths_redacted":redact,"source_payloads":false,"bounded_script_excerpts":true,"sample_assets":false,"credentials":false,"ciphertext":false},
            "logs":{"journal_limit_bytes":LOG_LIMIT,"rotations_retained":1,"inactive_retention_bytes":64*1024*1024,"inactive_retention_days":7},
            "issues":snapshot.events.iter().filter(|e| matches!(e.level, LogLevel::Warning | LogLevel::Error)).collect::<Vec<_>>(),
        });
        clean(&mut report, redact);
        let mut recent = Vec::new();
        for event in &snapshot.events {
            let mut value = serde_json::to_value(event)?;
            clean(&mut value, redact);
            serde_json::to_writer(&mut recent, &value)?;
            recent.push(b'\n');
        }
        write_new(&destination.join("events.jsonl"), &recent)?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination.join("journal.jsonl"))?;
        let mut rows = 0_u64;
        let mut malformed = 0_u64;
        let mut journal_warnings: Vec<String> = Vec::new();
        let (paths, _guards, active_omitted) = export_journals(journal, &mut journal_warnings);
        let mut source_sessions = Vec::new();
        for (path, source_session) in paths {
            let input = match File::open(&path) {
                Ok(input) => input,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    journal_warnings.push(format!("Could not read journal: {error}"));
                    continue;
                }
            };
            // The inactive-session retention policy is the historical export
            // byte ceiling, including older versions' slightly larger journals.
            if input
                .metadata()
                .map(|m| m.len())
                .unwrap_or(64 * 1024 * 1024 + 1)
                > 64 * 1024 * 1024
            {
                journal_warnings.push(
                    "Journal exceeds its configured byte limit or its metadata is unavailable"
                        .into(),
                );
                continue;
            }
            if !source_sessions.contains(&source_session) {
                source_sessions.push(source_session.clone());
            }
            for line in BufReader::new(input).lines() {
                if stopping.load(Ordering::Acquire) { return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "Support export canceled")); }
                let line = match line {
                    Ok(line) => line,
                    Err(error) => {
                        journal_warnings.push(format!("Journal read failed: {error}"));
                        break;
                    }
                };
                let mut value = match serde_json::from_str::<Value>(&line) {
                    Ok(value) => value,
                    Err(_) => {
                        malformed += 1;
                        continue;
                    }
                };
                if !value.is_object() {
                    malformed += 1;
                    continue;
                }
                clean(&mut value, redact);
                value["export_source_session_id"] = json!(source_session);
                serde_json::to_writer(&mut output, &value)?;
                output.write_all(b"\n")?;
                rows += 1;
            }
        }
        output.sync_all()?;
        let crash = crate::support::export_crash_evidence(destination, stopping)?;
        let crash_partial = crash.manifest["coverage"] != "complete_at_capture";
        journal_warnings.extend(crash.warnings);
        report["private_crash_evidence"] = crash.manifest;
        report["privacy"]["private_crash_originals_unredacted"] = json!(true);
        report["privacy"]["guarantees_scope"] = json!("structured_logs_only; exact private originals may contain sensitive native fields");
        let partial = crash_partial || flush_error.is_some()
            || snapshot.status.write_errors > 0
            || snapshot.status.dropped_events > 0
            || malformed > 0
            || !journal_warnings.is_empty();
        report["journal_coverage"] = json!(if partial {
            "partial"
        } else {
            "retained_history"
        });
        report["journal_rows_exported"] = json!(rows);
        report["malformed_journal_rows_omitted"] = json!(malformed);
        report["journal_warnings"] = json!(journal_warnings);
        report["flush_error"] = json!(flush_error);
        report["journal_source_sessions"] = json!(source_sessions);
        report["other_active_sessions_omitted"] = json!(active_omitted);
        clean(&mut report, redact);
        write_new(
            &destination.join("report.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        let readme = format!(
            "KONTRA diagnostic report\n\nBuild: {}\nStructured log paths redacted: {redact}\nRecent events: {} (bounded to {HISTORY_LIMIT} rows / {} bytes)\nSession events: {}\nDropped before journal: {}\nWrite errors: {}\nJournal rows exported: {rows}\nMalformed journal rows omitted: {malformed}\n\nreport.json: build, system, caller-provided host/audio configuration, recent issues and counters.\nevents.jsonl: bounded recent in-memory history, including events lost before disk.\njournal.jsonl: current journal and one retained rotation, in chronological order.\n\nJournals rotate before exceeding 8MiB; at most two files per active session.\nInactive sessions are pruned to 64MiB / 7 days; active sessions are never deleted.\nAbrupt process termination may lose queued events; stage-start records show the last observed operation.\nStructured logs include bounded script excerpts and exclude full scripts, sample assets, ciphertext and access credentials. These guarantees do not apply to exact private crash originals below.\nPath redaction preserves file basenames; review diagnostic messages and library names before sharing.\n\ncrash-evidence/manifest.json: per-source ownership and complete/partial/missing/active coverage.\ncrash-evidence/: exact archived private crash journals, native originals and saved incident/delivery records. These copies are UNREDACTED, even when structured-log path redaction is enabled, and may contain sensitive native fields. They are included only in this user-requested local export, never automatic uploads. The export does not search OS crash-report directories or copy binary process dumps. Missing, active or unavailable sources are labeled in the manifest, with retained source paths and available content hashes. Review before sharing.\n",
            snapshot.build["version"].as_str().unwrap_or("unknown"),
            snapshot.events.len(),
            HISTORY_BYTES,
            snapshot.status.total_events,
            snapshot.status.dropped_events,
            snapshot.status.write_errors,
        );
        let readme = format!(
            "{readme}\nJournal coverage: {}\nJournal warnings: {}\nFlush error: {}\nJournal includes retained inactive sessions from prior runs for crash diagnosis.\nSource sessions: {}\nOther active sessions omitted: {active_omitted}\n",
            report["journal_coverage"],
            report["journal_warnings"],
            report["flush_error"],
            report["journal_source_sessions"]
        );
        write_new(&destination.join("README.txt"), readme.as_bytes())?;
        let mut warnings = report["journal_warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        if snapshot.status.dropped_events > 0 {
            warnings.push(format!(
                "{} events were dropped before disk",
                snapshot.status.dropped_events
            ));
        }
        if snapshot.status.write_errors > 0 {
            warnings.push(format!(
                "{} diagnostic write errors",
                snapshot.status.write_errors
            ));
        }
        if malformed > 0 {
            warnings.push(format!("{malformed} malformed journal rows omitted"));
        }
        if let Some(error) = report["flush_error"].as_str() {
            warnings.push(format!("Journal flush: {error}"));
        }
        Ok(ExportCoverage { partial, warnings })
    })();
    if result.is_err() {
        // This directory was created by this job. Failed bundles remain marked
        // for inspection instead of deleting files at a caller-controlled path.
        let _ = write_new(
            &destination.join("INCOMPLETE.txt"),
            b"Export failed. This report is incomplete; do not treat it as a successful export.\n",
        );
    }
    result
}

fn truncate(value: &mut String, maximum: usize) {
    if value.len() <= maximum {
        return;
    }
    let mut at = maximum;
    while !value.is_char_boundary(at) {
        at -= 1;
    }
    value.truncate(at);
    value.push_str(" [truncated]");
}
fn prohibited(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        key.as_str(),
        "key"
            | "token"
            | "password"
            | "secret"
            | "source"
            | "source_code"
            | "script_source"
            | "script_text"
            | "sample_data"
            | "sample_bytes"
            | "sample_payload"
            | "ciphertext"
            | "plaintext"
            | "access_data"
    ) || key.contains("access_key")
        || key.contains("api_key")
        || key.contains("credential")
        || key.contains("private_key")
        || key.contains("encrypted_payload")
}
/// Sanitization is deliberately centralized so preview, report and both log
/// streams have the same privacy behavior. It does not read any asset files.
pub(crate) fn clean(value: &mut Value, redact_paths: bool) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !prohibited(key));
            for (key, value) in map {
                if redact_paths
                    && (key == "path"
                        || key.ends_with("_path")
                        || key == "resolved"
                        || key == "requested"
                        || key == "directory"
                        || key == "root")
                {
                    if let Some(s) = value.as_str() {
                        *value = Value::String(redact_path(s));
                        continue;
                    }
                }
                clean(value, redact_paths);
            }
        }
        Value::Array(array) => {
            for value in array {
                clean(value, redact_paths);
            }
        }
        Value::String(text) => {
            if redact_paths {
                *text = redact_text(text);
            }
        }
        _ => {}
    }
}
fn redact_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if !normalized.contains('/') {
        return path.into();
    }
    let basename = normalized
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    format!("<path>/{basename}")
}
fn redact_text(text: &str) -> String {
    // Replace absolute paths inside human-readable OS/decoder messages as well
    // as explicit JSON path fields. Spaces in quoted paths remain one token.
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        let previous = text[..at].chars().next_back();
        let boundary =
            previous.is_none_or(|p| p.is_whitespace() || matches!(p, '\'' | '"' | '(' | '=' | ':'));
        let windows = ch.is_ascii_alphabetic()
            && text[at..].as_bytes().get(1) == Some(&b':')
            && text[at..]
                .as_bytes()
                .get(2)
                .is_some_and(|b| matches!(b, b'/' | b'\\'));
        let unc = ch == '\\' && text[at..].starts_with("\\\\");
        if boundary && (ch == '/' || windows || unc) {
            let quote = previous.filter(|p| matches!(p, '\'' | '"'));
            let end = text[at..]
                .char_indices()
                .find(|(_, c)| {
                    if let Some(quote) = quote {
                        *c == quote
                    } else {
                        c.is_whitespace() || matches!(c, '\'' | '"' | ')' | ',')
                    }
                })
                .map(|(n, _)| at + n)
                .unwrap_or(text.len());
            result.push_str(&redact_path(&text[at..end]));
            while chars.peek().is_some_and(|(next, _)| *next < end) {
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// One instrument attempt; enqueue stage starts before blocking loader work.
/// A crash may lose queued rows; the last persisted start identifies the last
/// operation observed by the journal, without claiming every row reached disk.
pub struct LoadTrace {
    report: Value,
    journal_details: Value,
    detail_revision: u64,
    seen: std::collections::HashSet<(&'static str, &'static str, String)>,
    #[cfg(test)]
    test_session: Option<Arc<Session>>,
    started: Instant,
    stage_started: Instant,
    stage: &'static str,
    finished: bool,
}
impl LoadTrace {
    pub fn new(path: &Path, program: u32, part: Option<usize>) -> Self {
        let id = format!(
            "{}-{}-{}",
            std::process::id(),
            now(),
            NEXT_LOAD.fetch_add(1, Ordering::Relaxed)
        );
        let mut this = Self {
            report: json!({"load_id":id,"path":path,"program":program,"part":part,"version":env!("CARGO_PKG_VERSION"),"build_hash":env!("KONTRA_BUILD_HASH"),"import_hash":env!("KONTRA_IMPORT_HASH"),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"status":"loading","stages_ms":{},"details":{},"issues":[]}),
            journal_details: json!({}),
            detail_revision: 0,
            seen: Default::default(),
            #[cfg(test)]
            test_session: None,
            started: Instant::now(),
            stage_started: Instant::now(),
            stage: "",
            finished: false,
        };
        this.report["build"] = build_identity();
        this.report["schema_version"] = json!(SCHEMA_VERSION);
        this.record("load_started", json!({}));
        if let Some(path) = log_path() {
            this.report["log_path"] = json!(path);
        }
        this
    }
    fn record(&mut self, event: &str, data: Value) {
        let stage = data["stage"].as_str().unwrap_or(self.stage);
        let module = match stage {
            "scripts" => "ksp",
            "samples" | "preload" => "samples",
            "artwork" | "widgets" | "ui" => "ui",
            "effects" => "effects",
            "import" => "import",
            _ => "loader",
        };
        let row = json!({"event":event,"module":module,"load_id":self.report["load_id"],"path":self.report["path"],"program":self.report["program"],"part":self.report["part"],"instance_id":self.report["details"]["instance_id"],"library":self.report["details"]["library"],"stage":stage,"data":data});
        #[cfg(test)]
        let error = match &self.test_session {
            Some(session) => enqueue(session, row, true),
            None => enqueue(&session(), row, true),
        };
        #[cfg(not(test))]
        let error = enqueue(&session(), row, true);
        if let Some(error) = error {
            self.report["logging_error"] = json!(error);
        }
    }
    fn end_stage(&mut self) {
        if !self.stage.is_empty() {
            let ms = self.stage_started.elapsed().as_secs_f64() * 1000.;
            self.report["stages_ms"][self.stage] = json!(ms);
            self.record("stage_finished", json!({"elapsed_ms":ms}));
        }
    }
    pub fn stage(&mut self, name: &'static str) {
        self.end_stage();
        self.stage = name;
        self.stage_started = Instant::now();
        self.record("stage_started", json!({}));
    }
    pub fn fail(&mut self, message: impl Into<String>) {
        let mut message = message.into();
        truncate(&mut message, 4096);
        if self.report["failure"].as_str() == Some(message.as_str()) {
            return;
        }
        self.report["failure"] = json!(message);
        self.issue(self.stage, "failed", message);
    }
    pub fn detail(&mut self, key: &str, value: impl Into<Value>) {
        let value = value.into();
        let mut compact = value.clone();
        // Sanitize before partitioning, so credentials/source payloads cannot
        // hide inside an inventory. Typed items keep export path redaction valid.
        clean(&mut compact, false);
        self.detail_revision += 1;
        self.compact_detail(&mut vec![key.to_owned()], &mut compact);
        self.report["details"][key] = value;
        self.journal_details[key] = compact;
    }
    fn compact_detail(&mut self, field: &mut Vec<String>, value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    field.push(key.clone());
                    self.compact_detail(field, value);
                    field.pop();
                }
            }
            Value::Array(items) if serde_json::to_vec(&*items).unwrap().len() > DETAIL_BYTES => {
                for (index, item) in items.iter_mut().enumerate() {
                    field.push(index.to_string());
                    self.compact_detail(field, item);
                    field.pop();
                }
                let total = items.len();
                let (mut chunk, mut bytes, mut start, mut index) = (Vec::new(), 2, 0, 0);
                for item in std::mem::take(items) {
                    let size = serde_json::to_vec(&item).unwrap().len() + 1;
                    if !chunk.is_empty() && bytes + size > DETAIL_BYTES {
                        let count = chunk.len();
                        self.detail_chunk(field, index, start, total, chunk);
                        start += count;
                        index += 1;
                        chunk = Vec::new();
                        bytes = 2;
                    }
                    chunk.push(item);
                    bytes += size;
                }
                self.detail_chunk(field, index, start, total, chunk);
                *value = json!({"journal_inventory":{"field":field,"revision":self.detail_revision,"items":total,"chunks":index+1}});
            }
            _ => {}
        }
    }
    fn detail_chunk(&mut self, field: &[String], index: usize, start: usize, total: usize, items: Vec<Value>) {
        self.record("load_detail", json!({
            "code":"load_detail_inventory", "field":field, "revision":self.detail_revision,
            "chunk":index, "item_start":start, "items_total":total, "final":start+items.len()==total,
            "message":format!("Load detail {}: items {}..{} of {total}", field.join("."), start, start+items.len()),
            "items":items,
        }));
    }
    pub fn issue(&mut self, stage: &'static str, code: &'static str, message: impl Into<String>) {
        self.issue_details(stage, code, message.into(), json!({}));
    }
    /// Script errors retain their original message and a bounded local excerpt.
    /// Full script payloads are never recorded.
    pub fn script_issue(&mut self, code: &'static str, message: impl Into<String>, sources: &[String]) {
        let message = message.into();
        let (slot, line) = script_location(&message);
        let column = message.split_once("column ").or_else(|| message.split_once("col "))
            .and_then(|(_, text)| text.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u32>().ok())
            .filter(|&c| c > 0);
        let mut details = json!({});
        if let (Some(slot), Some(line)) = (slot, line) {
            details["script_slot"] = json!(slot);
            details["line"] = json!(line);
            if let Some(source) = sources.get(slot as usize)
                && let Some(excerpt) = script_excerpt(source, slot + 1, line, column)
            { details["source_excerpt"] = excerpt; }
        }
        self.issue_details("scripts", code, message, details);
    }
    fn issue_details(&mut self, stage: &'static str, code: &'static str, mut message: String, mut issue: Value) {
        truncate(&mut message, 4096);
        issue["stage"] = json!(stage);
        issue["code"] = json!(code);
        issue["message"] = json!(message);
        let key = (stage, code, message.clone());
        if self.seen.contains(&key) {
            return;
        }
        if self.seen.len() >= 1024 {
            let error = matches!(code, "failed" | "initialization_failed");
            if error && self.report["last_error"] == issue {
                return;
            }
            let omitted = self.report["issues_omitted"].as_u64().unwrap_or(0) + 1;
            self.report["issues_omitted"] = json!(omitted);
            if omitted == 1 {
                self.record("issue", json!({"stage":stage,"code":"diagnostics_truncated","message":"Load report issues retain the first 1024 distinct examples; additional occurrences are counted in issues_omitted. Additional warnings and errors wait for bounded worker queue capacity; recorder drop/write counters report delivery failure. The latest error is retained as last_error"}));
            }
            if error {
                self.report["last_error"] = issue.clone();
            }
            self.record("issue", issue);
            return;
        }
        self.seen.insert(key);
        self.report["issues"]
            .as_array_mut()
            .unwrap()
            .push(issue.clone());
        self.record("issue", issue);
    }
    pub fn finish(mut self, status: &str) -> Arc<Value> {
        self.end_stage();
        self.report["elapsed_ms"] = json!(self.started.elapsed().as_secs_f64() * 1000.);
        self.report["status"] = json!(if status == "loaded"
            && !self.report["issues"].as_array().unwrap().is_empty()
        {
            "partial"
        } else {
            status
        });
        self.finished = true;
        // Each issue already has a journal event. Repeating the full report can
        // exceed EVENT_BYTES and discard the useful timings/details summary.
        self.record(
            "load_finished",
            json!({
                "status":self.report["status"], "elapsed_ms":self.report["elapsed_ms"], "reason":self.report["failure"],
                "stages_ms":self.report["stages_ms"], "details":self.journal_details,
                "issues_retained":self.report["issues"].as_array().unwrap().len(),
                "issues_omitted":self.report["issues_omitted"].as_u64().unwrap_or(0),
                "last_error":self.report["last_error"],
            }),
        );
        Arc::new(self.report.clone())
    }
}
impl Drop for LoadTrace {
    fn drop(&mut self) {
        if !self.finished {
            self.report["status"] = json!("incomplete");
            self.record(
                "load_incomplete",
                json!({"status":"incomplete","elapsed_ms":self.started.elapsed().as_secs_f64()*1000.}),
            );
        }
    }
}

pub fn widget_limit(kind: &str) -> Option<&'static str> {
    match kind {
        "ui_table" => Some("table rendering is supported; table editing is unavailable"),
        "ui_text_edit" => Some("text display is supported; text editing is unavailable"),
        "ui_level_meter" => {
            Some("meter colours/frame are supported; live audio attachment is unavailable")
        }
        "ui_mouse_area" => Some("mouse-area callbacks are unavailable"),
        "ui_waveform" => {
            Some("waveform peaks and play cursor are supported; slice/table editing is unavailable")
        }
        "ui_file_selector" => Some("file selection is supported through the native picker; embedded columns and fs_navigate are unavailable"),
        "ui_xy" | "ui_wavetable" => {
            Some("this widget's drawing and interaction are unavailable")
        }
        _ => None,
    }
}

pub fn code(message: &str) -> &'static str {
    let s = message.to_ascii_lowercase();
    if s.contains("out of bounds") {
        "out_of_bounds"
    } else if s.contains("callback disabled") {
        "disabled_callback"
    } else if s.contains("unsupported")
        || s.contains("not implemented")
        || s.contains("unavailable")
    {
        "unsupported"
    } else if s.contains("not found") || s.contains("missing") {
        "missing"
    } else {
        "warning"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn manual_support_export_preserves_private_crash_originals_and_reports_missing_or_busy_sources() {
        const CHILD: &str = "KONTRA_MANUAL_CRASH_EXPORT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "diagnostics::tests::manual_support_export_preserves_private_crash_originals_and_reports_missing_or_busy_sources", "--test-threads=1"])
                .env(CHILD,"1").env("KONTRA_REPORT_DIR", directory.path().join("private-cache"))
                .env("KONTRA_DISABLE_NETWORK","1").status().unwrap();
            assert!(status.success()); return;
        }
        let cache=PathBuf::from(std::env::var_os("KONTRA_REPORT_DIR").unwrap());
        let root=cache.join("crash-reports");
        for name in ["originals","pending","deferred","sessions","panics"] { std::fs::create_dir_all(root.join(name)).unwrap(); }
        let id="0123456789abcdef";
        let original=vec![0x5a_u8;384*1024];
        let original_path=root.join("originals").join(format!("{id}.dfr"));
        std::fs::write(&original_path,&original).unwrap();
        let native=b"Native private original: /home/authored-user/library; private_key=authored-not-a-real-key\n".repeat(4096);
        let native_hash=blake3::hash(&native).to_hex().to_string();
        let native_path=root.join("originals").join(format!("{native_hash}.raw"));
        std::fs::write(&native_path,&native).unwrap();
        let mismatched_hash="e".repeat(64);
        let mismatched_native=root.join("originals").join(format!("{mismatched_hash}.raw"));
        std::fs::write(&mismatched_native,b"changed native content no longer matches saved identity").unwrap();
        std::fs::write(root.join("originals").join(format!("{native_hash}.json")),serde_json::to_vec(&json!({"status":"native-original-unverified","bytes":native.len(),"blake3":native_hash,"local_file":format!("originals/{native_hash}.raw"),"automatically_uploaded":false})).unwrap()).unwrap();
        let missing="fedcba9876543210";
        let missing_hash="d".repeat(64);
        let pending_bytes=serde_json::to_vec(&json!({"id":id,"local_journal":{"local_file":format!("originals/{missing}.dfr"),"bytes":384*1024,"blake3":missing_hash,"omitted_slots":87,"capture_error":"authored original unavailable"}})).unwrap();
        std::fs::write(root.join("pending.json"),&pending_bytes).unwrap();
        std::fs::write(root.join("pending").join(format!("{id}.json")),&pending_bytes).unwrap();
        std::fs::write(root.join("deferred-cursor.json"),b"7").unwrap();
        let pending_cursor=br#""0000000000000002.json""#;
        std::fs::write(root.join("pending-cursor.json"),pending_cursor).unwrap();
        let changed_journal=b"complete archived bytes retained despite recorded identity mismatch";
        for (changed_id, expected_bytes, expected_hash) in [("2222222222222222",changed_journal.len()+1,blake3::hash(changed_journal).to_hex().to_string()),("3333333333333333",changed_journal.len(),"a".repeat(64))] {
            std::fs::write(root.join("originals").join(format!("{changed_id}.dfr")),changed_journal).unwrap();
            std::fs::write(root.join("deferred").join(format!("{changed_id}.json")),serde_json::to_vec(&json!({"id":changed_id,"local_journal":{"local_file":format!("originals/{changed_id}.dfr"),"bytes":expected_bytes,"blake3":expected_hash}})).unwrap()).unwrap();
        }
        let deferred=root.join("deferred").join(format!("{id}.json"));
        let deferred_bytes=serde_json::to_vec(&json!({"id":id,"local_journal":{"local_file":format!("originals/{id}.dfr")}})).unwrap();
        std::fs::write(&deferred,&deferred_bytes).unwrap();
        let receipt=serde_json::to_vec(&json!({"incident_id":id,"report_id":"authored-receipt","status":"delivered"})).unwrap();
        std::fs::write(cache.join("last-report.json"),&receipt).unwrap();
        // An actual publisher lock holds only this source. Export must omit it
        // without waiting or losing any other evidence, then include it on retry.
        let publisher=File::options().create(true).truncate(false).read(true).write(true)
            .open(buffr_durable_file::lock_path(&deferred).unwrap()).unwrap();publisher.lock().unwrap();
        std::fs::write(root.join("sessions/live.json"),serde_json::to_vec(&json!({"pid":std::process::id(),"host_process":"current-host","journal_file":"live.dfr"})).unwrap()).unwrap();
        std::fs::write(root.join("sessions/live.dfr"),b"live source must not be copied").unwrap();
        std::fs::write(root.join("sessions/orphan.dfr"),b"unverified owner must not be copied").unwrap();
        std::fs::write(root.join("sessions/oversized-owner.json"),serde_json::to_vec(&json!({"pid":u32::MAX,"host_process":"x".repeat(4097),"journal_file":"oversized-owner.dfr"})).unwrap()).unwrap();
        std::fs::write(root.join("sessions/oversized-owner.dfr"),b"unverified oversized process identifier").unwrap();
        std::fs::write(root.join("sessions/dead.json"),serde_json::to_vec(&json!({"pid":u32::MAX,"host_process":"authored-dead-host","journal_file":"dead.dfr"})).unwrap()).unwrap();
        std::fs::write(root.join("sessions/dead.dfr"),&original).unwrap();
        std::fs::write(root.join("panics/4294967295.json"),b"{\"authored\":true}").unwrap();
        std::fs::write(root.join("deferred/1111111111111111.json"),br#"{"local_journal":{"local_file":"../outside-private.txt"}}"#).unwrap();
        let outside=cache.join("outside-private.txt");std::fs::write(&outside,b"outside ownership boundary").unwrap();
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&outside,root.join("originals").join(format!("{}.raw","f".repeat(64)))).unwrap();
        }
        let directory=tempfile::tempdir().unwrap();let logs=directory.path().join("logs");std::fs::create_dir(&logs).unwrap();
        let journal=logs.join("session-authored.jsonl");std::fs::write(&journal,b"{\"data\":{\"path\":\"/home/authored-user/library/file.nki\"}}\n").unwrap();
        let snapshot=DiagnosticSnapshot { revision:0,events:Vec::new(),status:LogStatus::default(),build:json!({"version":"authored"}) };
        let bundle=directory.path().join("report");
        let coverage=export_bundle(&bundle,&json!({"redact_paths":true}),&snapshot,&journal,None).unwrap();assert!(coverage.partial);
        let copied=bundle.join("crash-evidence/crash-reports/originals");
        assert_eq!(std::fs::read(copied.join(format!("{id}.dfr"))).unwrap(),original);
        assert_eq!(std::fs::read(copied.join(format!("{native_hash}.raw"))).unwrap(),native);
        assert_eq!(std::fs::read(bundle.join("crash-evidence/last-report.json")).unwrap(),receipt);
        assert_eq!(std::fs::read(bundle.join("crash-evidence/crash-reports/pending.json")).unwrap(),pending_bytes);
        assert_eq!(std::fs::read(bundle.join(format!("crash-evidence/crash-reports/pending/{id}.json"))).unwrap(),pending_bytes);
        assert_eq!(std::fs::read(bundle.join("crash-evidence/crash-reports/deferred-cursor.json")).unwrap(),b"7");
        assert_eq!(std::fs::read(bundle.join("crash-evidence/crash-reports/pending-cursor.json")).unwrap(),pending_cursor);
        assert_eq!(std::fs::read(bundle.join("crash-evidence/crash-reports/sessions/dead.dfr")).unwrap(),original);
        assert_eq!(std::fs::read(bundle.join("crash-evidence/crash-reports/panics/4294967295.json")).unwrap(),b"{\"authored\":true}");
        let manifest:Value=serde_json::from_slice(&std::fs::read(bundle.join("crash-evidence/manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["automatically_uploaded"],false);assert_eq!(manifest["paths_redacted"],false);
        let status=|path:&str|manifest["entries"].as_array().unwrap().iter().find(|e|e["source"]==path).unwrap()["status"].as_str().unwrap().to_owned();
        assert_eq!(status(&format!("crash-reports/originals/{missing}.dfr")),"missing");
        let missing_entry=manifest["entries"].as_array().unwrap().iter().find(|e|e["source"]==format!("crash-reports/originals/{missing}.dfr")).unwrap();
        assert_eq!(missing_entry["recorded_original"]["blake3"],missing_hash);
        assert_eq!(missing_entry["recorded_original"]["bytes"],384*1024);
        assert_eq!(missing_entry["recorded_original"]["omitted_slots"],87);
        assert_eq!(missing_entry["recorded_original"]["capture_error_present"],true);
        assert_eq!(status(&format!("crash-reports/originals/{mismatched_hash}.raw")),"incomplete");
        assert!(!copied.join(format!("{mismatched_hash}.raw")).exists());
        assert_eq!(std::fs::read(&mismatched_native).unwrap(),b"changed native content no longer matches saved identity");
        assert_eq!(status(&format!("crash-reports/deferred/{id}.json")),"publisher_active_omitted");
        assert_eq!(status("crash-reports/sessions/live.json"),"active_or_unverified_owner_omitted");
        assert_eq!(status("crash-reports/sessions/orphan.dfr"),"active_or_unverified_owner_omitted");
        for changed_id in ["2222222222222222","3333333333333333"] {
            assert_eq!(status(&format!("crash-reports/originals/{changed_id}.dfr")),"content_identity_mismatch");
            assert_eq!(std::fs::read(copied.join(format!("{changed_id}.dfr"))).unwrap(),changed_journal,"mismatched archived bytes remain available for manual diagnosis");
        }
        let oversized=manifest["entries"].as_array().unwrap().iter().find(|e|e["source"]=="crash-reports/sessions/oversized-owner.json").unwrap();
        assert_eq!(oversized["reason"],"owner_process_identifier_exceeds_4096_byte_limit");
        assert!(!bundle.join("crash-evidence/crash-reports/sessions/oversized-owner.dfr").exists());
        assert!(!bundle.join("crash-evidence/crash-reports/sessions/live.dfr").exists());
        assert!(!bundle.join("outside-private.txt").exists());
        assert_eq!(std::fs::read(&outside).unwrap(),b"outside ownership boundary");
        assert!(!std::fs::read_to_string(bundle.join("journal.jsonl")).unwrap().contains("authored-user"),"structured redaction remains active");
        assert!(std::fs::read_to_string(bundle.join("README.txt")).unwrap().contains("UNREDACTED"));
        assert!(export_preview(&json!({}))["privacy"].as_str().unwrap().contains("UNREDACTED"));
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(copied.join(format!("{id}.dfr"))).unwrap().permissions().mode()&0o777,0o600);
            assert_eq!(std::fs::metadata(bundle.join("crash-evidence")).unwrap().permissions().mode()&0o777,0o700);
        }
        assert!(export_bundle(&bundle,&json!({}),&snapshot,&journal,None).is_err(),"existing destinations cannot overwrite sources or exports");
        drop(publisher);
        let retry=directory.path().join("retry");export_bundle(&retry,&json!({}),&snapshot,&journal,None).unwrap();
        assert_eq!(std::fs::read(retry.join(format!("crash-evidence/crash-reports/deferred/{id}.json"))).unwrap(),deferred_bytes);
        let canceled=directory.path().join("canceled");
        assert!(export_bundle_with_cancel(&canceled,&json!({}),&snapshot,&journal,None,&AtomicBool::new(true)).is_err());
        assert!(canceled.join("INCOMPLETE.txt").exists());
        assert_eq!(std::fs::read(&original_path).unwrap(),original);assert_eq!(std::fs::read(&native_path).unwrap(),native);
        let overlap=cache.join("manual-report");assert!(export_bundle(&overlap,&json!({}),&snapshot,&journal,None).is_err());
        assert_eq!(std::fs::read(&original_path).unwrap(),original);
    }


    use super::*;

    #[test]
    fn load_detail_inventory_preserves_large_metadata_in_bounded_journal_and_export() {
        let directory = std::env::temp_dir().join(format!("kontra-detail-contract-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let session = Session::start(directory.join("logs"));
        let dependencies: Vec<Value> = (0..700).map(|n| json!({
            "path":format!("/private/inventory-owner/{}/Authored dependency {n}.wav", "directory-".repeat(12)),
            "version":[n,1790000000000000000u64+n],
        })).collect();
        assert!(serde_json::to_vec(&dependencies).unwrap().len() > 64*1024);
        let mut trace = LoadTrace::new(Path::new("Authored inventory.nki"), 0, None);
        trace.test_session = Some(session.clone());
        trace.stage("import");
        trace.detail("applied_instrument", json!({"name":"Authored inventory", "dependencies":dependencies}));
        let excerpt = json!({"script_slot":1,"line":3,"text":"3 | authored_array[700] := 1"});
        trace.issue_details("scripts", "warning", "Authored warning stays separate from the inventory".into(), json!({"source_excerpt":excerpt}));
        let report = trace.finish("loaded");
        assert_eq!(report["details"]["applied_instrument"]["dependencies"], json!(dependencies), "local report retains the complete original inventory");
        let (reply, done) = mpsc::channel();
        lock(&session.sender).as_ref().unwrap().send(Command::Flush(reply)).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        let read = |path: &Path| -> Vec<Value> {
            std::fs::read_to_string(path).unwrap().lines().map(|line| {
                assert!(line.len() <= EVENT_BYTES);
                serde_json::from_str(line).unwrap()
            }).collect()
        };
        let reassemble = |rows: &[Value]| -> Vec<Value> {
            let mut chunks: Vec<_> = rows.iter().filter(|row| row["event"] == "load_detail").collect();
            chunks.sort_by_key(|row| row["data"]["chunk"].as_u64().unwrap());
            let summary = rows.iter().find(|row| row["event"] == "load_finished").unwrap();
            let reference = &summary["data"]["details"]["applied_instrument"]["dependencies"]["journal_inventory"];
            assert_eq!(reference["items"], 700);
            assert_eq!(reference["chunks"].as_u64().unwrap(), chunks.len() as u64);
            let mut items = Vec::new();
            for (index, row) in chunks.iter().enumerate() {
                let data = &row["data"];
                assert_eq!(data["field"], json!(["applied_instrument","dependencies"]));
                assert_eq!(data["revision"], reference["revision"]);
                assert_eq!(data["chunk"], json!(index));
                assert_eq!(data["item_start"], json!(items.len()));
                assert_eq!(data["items_total"], 700);
                assert_eq!(data["final"], index+1 == chunks.len());
                items.extend(data["items"].as_array().unwrap().iter().cloned());
            }
            assert_eq!(items.len(), 700);
            items
        };
        let rows = read(&session.path);
        assert_eq!(reassemble(&rows), dependencies);
        assert!(rows.iter().any(|row| row["data"]["source_excerpt"] == excerpt), "inventory chunking preserves separate offending code excerpts");
        let history = lock(&session.history);
        assert_eq!((history.status.dropped_events, history.status.write_errors, history.status.truncated_events), (0,0,0));
        assert_eq!(history.status.journal_events_written, rows.len() as u64);
        let snapshot = DiagnosticSnapshot { revision:revision(), events:history.events.iter().map(|(event,_)|event.clone()).collect(), status:history.status.clone(), build:build_identity() };
        drop(history);
        let bundle = directory.join("bundle");
        export_bundle(&bundle, &json!({"load":report.as_ref()}), &snapshot, &session.path, None).unwrap();
        let exported = read(&bundle.join("journal.jsonl"));
        assert!(exported.iter().any(|row| row["data"]["source_excerpt"] == excerpt));
        let mut redacted = json!(dependencies);
        clean(&mut redacted,true);
        assert_eq!(json!(reassemble(&exported)), redacted);
        assert!(!std::fs::read_to_string(bundle.join("journal.jsonl")).unwrap().contains("/private/inventory-owner"));
        let mut owner = Some(session.clone());
        stop(&mut owner).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn load_warning_journal_retains_records_beyond_report_example_limit() {
        let directory = std::env::temp_dir().join(format!("kontra-load-warning-contract-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let session = Session::start(directory.join("logs"));
        let mut trace = LoadTrace::new(Path::new("Authored warning fixture.nki"), 0, None);
        // An isolated real worker/journal avoids unrelated parallel producers
        // changing this contract's queue pressure or rotating its history away.
        trace.test_session = Some(session.clone());
        let flush = || {
            let (reply, done) = mpsc::channel();
            lock(&session.sender).as_ref().unwrap().send(Command::Flush(reply)).unwrap();
            done.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        };
        // Pause the real disk worker, fill its queue, then let the load worker
        // continue through backpressure. No intermediate flush hides burst loss.
        let (entered, paused) = mpsc::channel();
        let (resume, resumed) = mpsc::channel();
        lock(&session.sender).as_ref().unwrap().send(Command::Pause(entered, resumed)).unwrap();
        paused.recv_timeout(Duration::from_secs(5)).unwrap();
        let (filled, full) = mpsc::channel();
        let producer = std::thread::spawn(move || {
            for n in 0..4098 {
                trace.issue("effects", "unsupported_group_effect", format!("Group {n}: authored unsupported effect"));
                if n == QUEUE_LIMIT - 1 { filled.send(()).unwrap(); }
            }
            trace.issue("effects", "unsupported_group_effect", "Group 0: authored unsupported effect");
            trace.finish("loaded")
        });
        full.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(lock(&session.history).status.total_events <= (QUEUE_LIMIT + 2) as u64, "the load worker waits on the bounded queue");
        resume.send(()).unwrap();
        let report = producer.join().unwrap();
        flush();
        assert_eq!(report["issues"].as_array().unwrap().len(), 1024);
        assert_eq!(report["issues_omitted"], 3074, "known duplicates are not omitted examples");
        assert_eq!(report["status"], "partial");
        let journal = std::fs::read_to_string(&session.path).unwrap();
        let records: Vec<Value> = journal.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        let warnings: Vec<_> = records.iter().filter(|row| row["code"] == "unsupported_group_effect").collect();
        assert_eq!(warnings.len(), 4098, "every distinct warning reaches the real journal after the report cap");
        for n in [0, 1023, 1024, 4097] {
            let message = format!("Group {n}: authored unsupported effect");
            assert_eq!(warnings.iter().filter(|row| row["reason"] == message).count(), 1);
        }
        let history = lock(&session.history);
        assert!(history.events.len() <= HISTORY_LIMIT);
        assert_eq!((history.status.dropped_events, history.status.write_errors), (0, 0));
        assert_eq!(history.status.journal_events_written, records.len() as u64);
        assert!(history.status.history_evicted > 0);
        let snapshot = DiagnosticSnapshot {
            revision: revision(), events: history.events.iter().map(|(event, _)| event.clone()).collect(),
            status: history.status.clone(), build: build_identity(),
        };
        drop(history);
        let bundle = directory.join("bundle");
        export_bundle(&bundle, &json!({"load":report.as_ref()}), &snapshot, &session.path, None).unwrap();
        let exported = std::fs::read_to_string(bundle.join("journal.jsonl")).unwrap();
        assert_eq!(exported.lines().filter(|line| serde_json::from_str::<Value>(line).unwrap()["code"] == "unsupported_group_effect").count(), 4098, "support export includes warnings omitted only from the load summary");
        let mut owner = Some(session.clone());
        stop(&mut owner).unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn journal_contract_bounds_rotates_recovers_and_exports_private_reports() {
        let directory = std::env::temp_dir().join(format!(
            "kontra-diagnostic-contract-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let session = Session::start(directory.join("logs"));
        let source = format!("{}set_key_color(128,$KEY_COLOR_RED)\nend on", "\n".repeat(36));
        let row = json!({"module":"ksp","event":"runtime_issue","load_id":"load-contract","path":"/private/alice/Harp.nki","data":{"slot":2,"line":37,"message":"Cannot read '/private/alice/Missing Sample.wav'","count":4,"script_epoch":9,"source_excerpt":script_excerpt(&source,2,37,None)}});
        emit_to(&session, row.clone());
        let (reply, done) = mpsc::channel();
        lock(&session.sender)
            .as_ref()
            .unwrap()
            .send(Command::Flush(reply))
            .unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        let journal = std::fs::read_to_string(&session.path).unwrap();
        let started: Value = serde_json::from_str(journal.lines().next().unwrap()).unwrap();
        assert_eq!(started["event"], "session_started");
        assert_eq!(started["data"]["build"], build_identity());
        let event = lock(&session.history).events.back().unwrap().0.clone();
        assert_eq!(
            (event.script_slot, event.line, event.script_epoch),
            (Some(2), Some(37), Some(9))
        );
        assert_eq!(event.level, LogLevel::Warning);
        assert!(event.timestamp_ms > 0);
        assert!(
            std::fs::read_to_string(&session.path)
                .unwrap()
                .contains("load-contract")
        );

        let huge = "😀".repeat(EVENT_BYTES);
        emit_to(
            &session,
            json!({"module":huge,"event":"oversized","path":huge,"load_id":huge,"data":{"message":huge,"library":huge,"payload":huge}}),
        );
        {
            let history = lock(&session.history);
            let row = &history.events.back().unwrap().0;
            assert!(serde_json::to_vec(row).unwrap().len() < EVENT_BYTES);
            assert_eq!(history.status.truncated_events, 1);
            assert_eq!(row.details["diagnostic_truncated"], true);
        }
        for _ in 0..HISTORY_LIMIT + 8 {
            emit_to(&session, row.clone());
        }
        let history = lock(&session.history);
        assert!(history.events.len() <= HISTORY_LIMIT);
        assert!(history.bytes <= HISTORY_BYTES);
        assert!(history.status.history_evicted > 0);
        assert_eq!(
            history.status.level_counts.iter().sum::<u64>(),
            history.status.total_events
        );
        drop(history);

        // A full queue counts loss without blocking producers; retained events
        // remain exportable even when none of them can reach disk.
        let (blocked, _receiver) = mpsc::sync_channel(1);
        let stalled = Session {
            id: "stalled".into(),
            path: directory.join("stalled.jsonl"),
            started: Instant::now(),
            history: Arc::new(Mutex::new(History {
                events: VecDeque::new(),
                bytes: 0,
                status: LogStatus::default(),
            })),
            sender: Mutex::new(Some(blocked)),
            worker: Mutex::new(None),
            export_stopping: Arc::new(AtomicBool::new(false)),
        };
        emit_to(&stalled, row.clone());
        emit_to(&stalled, row.clone());
        assert_eq!(lock(&stalled.history).status.dropped_events, 1);

        let (reply, done) = mpsc::channel();
        lock(&session.sender)
            .as_ref()
            .unwrap()
            .send(Command::Flush(reply))
            .unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        let history = lock(&session.history);
        let report_snapshot = DiagnosticSnapshot {
            revision: revision(),
            events: history.events.iter().map(|(e, _)| e.clone()).collect(),
            status: history.status.clone(),
            build: build_identity(),
        };
        drop(history);
        let mut journal = Journal::new(
            directory.join("logs/session-rotation.jsonl"),
            Arc::new(Mutex::new(History {
                events: VecDeque::new(),
                bytes: 0,
                status: LogStatus::default(),
            })),
        );
        journal.open().unwrap();
        journal
            .file
            .as_ref()
            .unwrap()
            .set_len(LOG_LIMIT - 1)
            .unwrap();
        journal.bytes = LOG_LIMIT - 1;
        journal.write_event(&event).unwrap();
        assert!(
            std::fs::metadata(journal.path.with_extension("previous.jsonl"))
                .unwrap()
                .len()
                <= LOG_LIMIT
        );
        assert!(std::fs::metadata(&journal.path).unwrap().len() < LOG_LIMIT);

        // Simulate a partial write and prove reopen uses append rather than
        // create_new on an existing journal or merging two JSON records.
        journal.file.take();
        let mut partial = OpenOptions::new().append(true).open(&journal.path).unwrap();
        partial.write_all(b"{broken").unwrap();
        drop(partial);
        journal.needs_separator = true;
        journal.error(std::io::Error::other("test partial write"));
        journal.write_event(&event).unwrap();
        assert_eq!(
            journal.bytes,
            std::fs::metadata(&journal.path).unwrap().len()
        );
        assert!(
            journal.flush().is_err(),
            "historical write failures cannot become successful flushes"
        );
        let lines = std::fs::read_to_string(&journal.path).unwrap();
        assert!(lines.lines().last().unwrap().starts_with('{'));
        assert_eq!(
            serde_json::from_str::<LogEvent>(lines.lines().last().unwrap())
                .unwrap()
                .load_id,
            Some("load-contract".into())
        );

        let mut private = json!({"path":"/private/alice/Harp.nki","nested":{"message":"open 'C:\\Users\\Alice\\Missing Sample.wav' failed","access_key":"secret-data","ciphertext":[1,2],"source":"script-body","sample_bytes":[3,4]},"audio":{"sample_rate":48000,"block_size":128}});
        clean(&mut private, true);
        assert!(!private.to_string().contains("Alice"));
        assert!(!private.to_string().contains("alice"));
        assert!(!private.to_string().contains("secret-data"));
        assert!(!private.to_string().contains("script-body"));
        assert_eq!(private["audio"]["sample_rate"], 48000);
        let bundle = directory.join("bundle");
        std::fs::write(directory.join("logs/session-crash.jsonl"), b"{\"event\":\"load_started\",\"load_id\":\"previous-crash\",\"stage\":\"samples\",\"path\":\"/private/alice/Harp.nki\"}\n\"valid JSON but not an event object\"\n").unwrap();
        let orphan = directory.join("logs/session-orphan.previous.jsonl");
        std::fs::write(
            &orphan,
            b"{\"event\":\"stage_started\",\"load_id\":\"orphan-crash\",\"stage\":\"samples\"}\n",
        )
        .unwrap();
        assert!(!orphan.with_file_name("session-orphan.jsonl").exists());
        let catalog = session_catalog(&directory.join("logs")).unwrap();
        assert_eq!(
            catalog.iter().filter(|p| **p == journal.path).count(),
            1,
            "primary plus rotation must map to one session"
        );
        export_bundle(
            &bundle,
            &private,
            &report_snapshot,
            &journal.path,
            Some("simulated flush failure".into()),
        )
        .unwrap();
        let report: Value =
            serde_json::from_slice(&std::fs::read(bundle.join("report.json")).unwrap()).unwrap();
        assert_eq!(report["journal_coverage"], "partial");
        assert_eq!(report["privacy"]["bounded_script_excerpts"], true);
        for file in ["events.jsonl", "journal.jsonl"] {
            let rows = std::fs::read_to_string(bundle.join(file)).unwrap();
            assert!(rows.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|event| excerpt_text(&event).is_some_and(|text| text.contains(">     37 | set_key_color(128,$KEY_COLOR_RED)"))), "{file} retains authorized source context");
        }
        assert!(
            report["journal_source_sessions"]
                .as_array()
                .unwrap()
                .contains(&json!("crash"))
        );
        assert!(
            std::fs::read_to_string(bundle.join("journal.jsonl"))
                .unwrap()
                .contains("previous-crash")
        );
        assert!(
            std::fs::read_to_string(bundle.join("journal.jsonl"))
                .unwrap()
                .contains("orphan-crash")
        );
        assert!(
            report["journal_source_sessions"]
                .as_array()
                .unwrap()
                .contains(&json!("orphan"))
        );
        File::options()
            .write(true)
            .open(&orphan)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60))
            .unwrap();
        retain_sessions(&directory.join("logs"), &session.path).unwrap();
        assert!(
            !orphan.exists(),
            "orphan rotations must honor age retention"
        );
        File::create(&orphan)
            .unwrap()
            .set_len(65 * 1024 * 1024)
            .unwrap();
        retain_sessions(&directory.join("logs"), &session.path).unwrap();
        assert!(
            !orphan.exists(),
            "orphan rotations must count toward the byte budget"
        );
        assert!(report["malformed_journal_rows_omitted"].as_u64().unwrap() >= 1);
        for name in ["report.json", "events.jsonl", "journal.jsonl", "README.txt"] {
            let text = std::fs::read_to_string(bundle.join(name)).unwrap();
            assert!(!text.contains("/private/alice"));
            assert!(!text.contains("C:\\Users\\Alice"));
        }
        assert!(
            export_bundle(&bundle, &private, &report_snapshot, &journal.path, None).is_err(),
            "existing user directories must not be overwritten"
        );

        // Inactive oversized sessions are pruned, while locked sessions survive
        // regardless of age/size. Native locks also work across plugin instances.
        let inactive = directory.join("logs/session-old.jsonl");
        File::create(&inactive)
            .unwrap()
            .set_len(65 * 1024 * 1024)
            .unwrap();
        retain_sessions(&directory.join("logs"), &session.path).unwrap();
        assert!(!inactive.exists());
        assert!(
            journal.path.exists(),
            "retention must leave locked sessions alone"
        );
        journal.close();
        let sender = lock(&session.sender).take().unwrap();
        sender.send(Command::Stop).unwrap();
        drop(sender);
        lock(&session.worker).take().unwrap().join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();

        // Preserve existing load outcomes/stage semantics without requiring a
        // successful instrument. Filter by this load ID when tests run in parallel.
        let mut trace = LoadTrace::new(Path::new("Missing Harp.nki"), 2, Some(3));
        trace.stage("import");
        trace.detail("zones", 17);
        trace.issue("artwork", "missing", "knob.png not found");
        let report = trace.finish("loaded");
        assert_eq!(report["status"], "partial");
        assert_eq!(report["details"]["zones"], 17);
        let recent = snapshot();
        assert!(
            recent
                .events
                .iter()
                .any(|row| row.load_id.as_deref() == report["load_id"].as_str()
                    && row.stage.as_deref() == Some("artwork")
                    && row.code.as_deref() == Some("missing"))
        );
        // Initialization's two existing text formatters retain their original
        // reasons while promoting the same zero-based slot and source line.
        let mut trace = LoadTrace::new(Path::new("Partial Harp.nki"), 0, Some(1));
        trace.stage("scripts");
        trace.detail("zones_total", 95_624);
        trace.detail("library", "Example Library");
        trace.issue(
            "scripts",
            "initialization_failed",
            "Script 2: KSP line 37: wait() is not allowed in on init",
        );
        trace.issue(
            "scripts",
            "unsupported",
            "Slot 3 line 42: unsupported effect (4x)",
        );
        for n in 0..12 {
            trace.issue(
                "samples",
                "zone_skipped",
                format!("Zone {n}: {}", "x".repeat(4096)),
            );
        }
        for n in 14..1024 {
            trace.issue("samples", "zone_skipped", format!("Additional zone {n}"));
        }
        trace.issue("samples", "zone_skipped", "Additional zone 14");
        trace.issue("samples", "zone_skipped", "Extra omitted zone");
        trace.issue(
            "scripts",
            "initialization_failed",
            "Script 4: KSP line 91: initialization stopped",
        );
        trace.issue(
            "scripts",
            "initialization_failed",
            "Script 4: KSP line 91: initialization stopped",
        );
        trace.fail("Cannot finish sample headers");
        let report = trace.finish("failed");
        assert_eq!(
            report["issues_omitted"], 3,
            "known duplicates are not counted as lost examples"
        );
        assert_eq!(report["failure"], "Cannot finish sample headers");
        assert!(serde_json::to_vec(report.as_ref()).unwrap().len() > EVENT_BYTES);
        let recent = snapshot();
        let events: Vec<_> = recent
            .events
            .iter()
            .filter(|row| row.load_id.as_deref() == report["load_id"].as_str())
            .collect();
        let init = events
            .iter()
            .find(|row| row.code.as_deref() == Some("initialization_failed"))
            .unwrap();
        assert_eq!(
            (init.script_slot, init.line, init.level),
            (Some(1), Some(37), LogLevel::Error)
        );
        assert_eq!(
            init.reason.as_deref(),
            Some("Script 2: KSP line 37: wait() is not allowed in on init")
        );
        let fault = events
            .iter()
            .find(|row| row.code.as_deref() == Some("unsupported"))
            .unwrap();
        assert_eq!((fault.script_slot, fault.line), (Some(2), Some(42)));
        let summary = events
            .iter()
            .find(|row| row.event == "load_finished")
            .unwrap();
        assert_eq!(summary.details["details"]["zones_total"], 95_624);
        assert_eq!(summary.details["issues_retained"], 1024);
        assert_eq!(summary.details["issues_omitted"], 3);
        assert_eq!(
            summary.details["last_error"]["message"],
            "Cannot finish sample headers"
        );
        assert!(
            events.iter().any(|row| row.script_slot == Some(3)
                && row.line == Some(91)
                && row.level == LogLevel::Error),
            "partial initialization errors are journaled even after the example budget fills"
        );
        assert_eq!(
            summary.reason.as_deref(),
            Some("Cannot finish sample headers")
        );
        assert_eq!(summary.level, LogLevel::Error);
        assert!(
            events
                .iter()
                .any(|row| row.code.as_deref() == Some("failed")
                    && row.reason.as_deref() == Some("Cannot finish sample headers")),
            "terminal causes remain immediately visible when warning examples fill the report"
        );
        assert!(summary.details["stages_ms"]["scripts"].is_number());
        assert!(summary.details.get("issues").is_none());
        assert!(summary.details.get("diagnostic_truncated").is_none());
        assert_eq!(summary.library.as_deref(), Some("Example Library"));

        for status in ["canceled", "stale"] {
            let trace = LoadTrace::new(Path::new("Harp.nki"), 0, None);
            assert_eq!(trace.finish(status)["status"], status);
        }
        let mut trace = LoadTrace::new(Path::new("unfinished.nki"), 0, None);
        trace.stage("samples");
        let id = trace.report["load_id"].as_str().unwrap().to_owned();
        drop(trace);
        assert!(
            snapshot()
                .events
                .iter()
                .any(|row| row.load_id.as_deref() == Some(&id)
                    && row.event == "load_incomplete"
                    && row.outcome.as_deref() == Some("incomplete"))
        );
    }
}
