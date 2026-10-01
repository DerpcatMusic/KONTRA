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
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const SCHEMA_VERSION: u32 = 1;
pub const HISTORY_LIMIT: usize = 2048;
const HISTORY_BYTES: usize = 2 * 1024 * 1024;
const EVENT_BYTES: usize = 32 * 1024;
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
            .or_else(|| crate::cache::dir().map(|p| p.join("logs")))
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
        // Creation, serialization, rotation and export all happen off UI/audio.
        let worker = std::thread::Builder::new()
            .name("kontra-diagnostics".into())
            .spawn(move || {
                let mut journal = Journal::new(worker_path, worker_history);
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Event(event) => {
                            if let Err(error) = journal.write_event(&event) {
                                journal.error(error);
                            }
                        }
                        Command::Flush(reply) => {
                            let _ = reply.send(journal.flush().map_err(|e| e.to_string()));
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
                            let result = export_bundle(
                                &path,
                                &context,
                                &snapshot,
                                &journal.path,
                                flush_error,
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
            started: Instant::now(),
            history,
            sender: Mutex::new(sender),
            worker: Mutex::new(worker),
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
/// `details`; never pass sample bytes, ciphertext, access data or source payloads.
pub fn event(level: LogLevel, module: &str, code: &str, details: Value) {
    emit(json!({"level":level,"module":module,"event":code,"code":code,"data":details}));
}
fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}
fn emit(value: Value) -> Option<String> {
    let session = session();
    emit_to(&session, value)
}
fn emit_to(session: &Session, value: Value) -> Option<String> {
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
            if data["code"] == "failed" {
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
        monotonic_ms: session.started.elapsed().as_millis() as u64,
        session_id: session.id.clone(),
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
    let mut history = lock(&session.history);
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
    if truncated {
        history.status.truncated_events += 1;
    }
    history.status.total_events += 1;
    history.status.level_counts[level.index()] += 1;
    row.sequence = REVISION.fetch_add(1, Ordering::AcqRel) + 1;
    history.bytes += bytes;
    history.events.push_back((row.clone(), bytes));
    while history.events.len() > HISTORY_LIMIT || history.bytes > HISTORY_BYTES {
        if let Some((_, bytes)) = history.events.pop_front() {
            history.bytes -= bytes;
            history.status.history_evicted += 1;
        }
    }
    let result = lock(&session.sender)
        .as_ref()
        .map(|s| s.try_send(Command::Event(row)));
    match result {
        Some(Ok(())) => history.status.last_error.clone(),
        Some(Err(TrySendError::Full(_))) => {
            history.status.dropped_events += 1;
            Some("Diagnostics queue is full; event retained in recent history only".into())
        }
        _ => {
            history.status.dropped_events += 1;
            Some("Diagnostics worker is unavailable".into())
        }
    }
}

/// Called after a diagnostic snapshot returns from the audio thread.
pub(crate) fn runtime(
    path: &Path,
    program: u32,
    part: usize,
    epoch: u64,
    load_id: Option<&str>,
    issues: &[Value],
) {
    for issue in issues {
        let mut data = issue.clone();
        if data.is_object() {
            data["script_epoch"] = json!(epoch);
        }
        emit(
            json!({"event":"runtime_issue","load_id":load_id,"path":path,"program":program,"part":part,"script_epoch":epoch,"data":data}),
        );
    }
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
        "contents":["report.json","README.txt","events.jsonl","journal.jsonl (current and retained inactive sessions)"],
        "privacy":"Source/script payloads, sample bytes, ciphertext and access credentials are excluded. Paths are redacted by default. Review the preview before sharing.",
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
fn export_bundle(
    destination: &Path,
    context: &Value,
    snapshot: &DiagnosticSnapshot,
    journal: &Path,
    flush_error: Option<String>,
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
            "privacy":{"paths_redacted":redact,"source_payloads":false,"sample_assets":false,"credentials":false,"ciphertext":false},
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
        let partial = flush_error.is_some()
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
            "KONTRA diagnostic report\n\nBuild: {}\nPaths redacted: {redact}\nRecent events: {} (bounded to {HISTORY_LIMIT} rows / {} bytes)\nSession events: {}\nDropped before journal: {}\nWrite errors: {}\nJournal rows exported: {rows}\nMalformed journal rows omitted: {malformed}\n\nreport.json: build, system, caller-provided host/audio configuration, recent issues and counters.\nevents.jsonl: bounded recent in-memory history, including events lost before disk.\njournal.jsonl: current journal and one retained rotation, in chronological order.\n\nJournals rotate before exceeding 8MiB; at most two files per active session.\nInactive sessions are pruned to 64MiB / 7 days; active sessions are never deleted.\nAbrupt process termination may lose queued events; stage-start records show the last observed operation.\nSource/script payloads, sample assets, ciphertext and access credentials are excluded.\nPath redaction preserves file basenames; review diagnostic messages and library names before sharing.\n",
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
fn clean(value: &mut Value, redact_paths: bool) {
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
    seen: std::collections::HashSet<(&'static str, &'static str, String)>,
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
            seen: Default::default(),
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
            "artwork" | "widgets" => "ui",
            "import" => "import",
            _ => "loader",
        };
        if let Some(error) = emit(
            json!({"event":event,"module":module,"load_id":self.report["load_id"],"path":self.report["path"],"program":self.report["program"],"part":self.report["part"],"instance_id":self.report["details"]["instance_id"],"stage":stage,"data":data}),
        ) {
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
        self.issue(self.stage, "failed", message);
    }
    pub fn detail(&mut self, key: &str, value: impl Into<Value>) {
        self.report["details"][key] = value.into();
    }
    pub fn issue(&mut self, stage: &'static str, code: &'static str, message: impl Into<String>) {
        let mut message = message.into();
        truncate(&mut message, 4096);
        if self.seen.len() >= 1024 {
            let omitted = self.report["issues_omitted"].as_u64().unwrap_or(0) + 1;
            self.report["issues_omitted"] = json!(omitted);
            if omitted == 1 {
                self.record("issue", json!({"stage":stage,"code":"diagnostics_truncated","message":"Load report retains the first 1024 distinct issue examples; additional examples are counted in issues_omitted"}));
            }
            return;
        }
        if !self.seen.insert((stage, code, message.clone())) {
            return;
        }
        let issue = json!({"stage":stage,"code":code,"message":message});
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
        self.record("load_finished", self.report.clone());
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
        "ui_xy" | "ui_wavetable" | "ui_file_selector" => {
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
    use super::*;

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
        let row = json!({"module":"ksp","event":"runtime_issue","load_id":"load-contract","path":"/private/alice/Harp.nki","data":{"slot":2,"line":37,"message":"Cannot read '/private/alice/Missing Sample.wav'","count":4,"script_epoch":9}});
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
