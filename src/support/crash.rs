// Adapted from BUFFR fd2fdba92f3f71cee24c0c72a39aa190fc9ee414; ISC, see LICENSE-BUFFR.
use super::MutexExt as _;
use super::platform::comparable_process_name;
use derpcat_flight_recorder::{Record as DiagnosticRecord, Recorder, RtEvent, RtWriter, Sink};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, mpsc};

const EVENT_CHANNEL_CAPACITY: usize = 512;
const RT_CHANNEL_CAPACITY: usize = 128;
// Preserve the complete session on the existing journal worker.
const JOURNAL_BYTES: usize = 0;
const RECOVERED_HEAD_RECORDS: usize = 128;
const RECOVERED_TAIL_RECORDS: usize = 2048;

#[derive(Clone, Default, Deserialize, Serialize)]
struct LocalJournalEvidence {
    bytes: u64,
    blake3: String,
    valid_slots: u64,
    omitted_slots: u64,
    local_file: String,
    #[serde(default)]
    capture_error: Option<String>,
}
#[cfg(test)]
const MAX_DIAGNOSTIC_EVENTS: usize = 256;
const MAX_EVIDENCE_CHARS: usize = 32_000;
#[derive(Clone, Deserialize, Serialize)]
struct SessionMarker {
    schema: u8,
    session_id: String,
    pid: u32,
    version: String,
    #[serde(default)]
    build_id: String,
    started_at: u64,
    host_process: String,
    os: String,
    architecture: String,
    #[serde(default)]
    host_name: String,
    #[serde(default)]
    plugin_api: String,
    #[serde(default)]
    platform: super::platform::PlatformSnapshot,
    #[serde(default)]
    journal_file: Option<String>,
    #[serde(default, alias = "breadcrumbs")]
    events: VecDeque<DiagnosticRecord>,
    #[serde(default)]
    dropped_events: u64,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    active_instances: BTreeSet<u64>,
    #[serde(default)]
    instances: BTreeMap<u64, InstanceLifecycle>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum InstanceLifecycle {
    #[default]
    Registered,
    Initializing,
    Active,
    Deactivating,
    Inactive,
}

impl SessionMarker {
    fn apply_lifecycle(
        &mut self,
        instance_id: u64,
        lifecycle: InstanceLifecycle,
        host_name: Option<String>,
        plugin_api: Option<String>,
    ) {
        self.instances.insert(instance_id, lifecycle);
        if let Some(host_name) = host_name {
            self.host_name = host_name;
        }
        if let Some(plugin_api) = plugin_api {
            self.plugin_api = plugin_api;
        }
    }
}

impl InstanceLifecycle {
    const fn is_unclean_if_process_dies(self) -> bool {
        matches!(self, Self::Initializing | Self::Active | Self::Deactivating)
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct PanicMarker {
    at: u64,
    thread: String,
    message: String,
    location: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    images: Vec<LoadedImage>,
}

/// One Mach-O image loaded in the crashing process.
///
/// The panic backtrace records raw instruction addresses. A stripped release
/// build resolves those to the nearest exported symbol, which names an
/// unrelated function. These fields are what an offline symbolizer needs:
/// `atos -o <dSYM binary> -l <load_address> <address>`, or `llvm-symbolizer
/// --obj=<dSYM binary> --adjust-vma=<slide>`. `uuid` selects the matching dSYM
/// from the ones retained by the signed build
/// (`scripts/ci/bundle-macos-universal.sh`).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct LoadedImage {
    path: String,
    load_address: u64,
    slide: i64,
    uuid: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CrashIncident {
    pub id: String,
    pub version: String,
    #[serde(default)]
    build_id: String,
    pub(crate) started_at: u64,
    pub(crate) detected_at: u64,
    pub(crate) host_process: String,
    pid: u32,
    os: String,
    architecture: String,
    #[serde(default)]
    host_name: String,
    #[serde(default)]
    plugin_api: String,
    #[serde(default)]
    platform: super::platform::PlatformSnapshot,
    #[serde(default, alias = "breadcrumbs")]
    events: VecDeque<DiagnosticRecord>,
    #[serde(default)]
    dropped_events: u64,
    #[serde(default)]
    panic: Option<PanicMarker>,
    #[serde(default)]
    kind: IncidentKind,
    #[serde(default)]
    evidence_signature: String,
    #[serde(default)]
    platform_evidence: String,
    #[serde(default)]
    source_schema: u8,
    #[serde(default)]
    host_finished_unload: bool,
    #[serde(default)]
    local_journal: Option<LocalJournalEvidence>,
}

/// A recovered incident with the given provenance, for tests in this module's
/// parent that need one without touching the filesystem.
#[cfg(test)]
pub(super) fn test_incident(version: &str, build_id: &str) -> CrashIncident {
    CrashIncident {
        id: "incident-1".to_string(),
        version: version.to_string(),
        build_id: build_id.to_string(),
        started_at: 1,
        detected_at: 2,
        host_process: "reaper.exe".to_string(),
        pid: 1,
        os: "windows".to_string(),
        architecture: "x86_64".to_string(),
        host_name: "REAPER".to_string(),
        plugin_api: "CLAP".to_string(),
        platform: super::platform::PlatformSnapshot::default(),
        events: VecDeque::new(),
        dropped_events: 0,
        panic: None,
        kind: IncidentKind::UncleanExit,
        evidence_signature: "sig".to_string(),
        platform_evidence: String::new(),
        source_schema: 5,
        host_finished_unload: false,
        local_journal: None,
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IncidentKind {
    Panic,
    PlatformCrash,
    #[default]
    UncleanExit,
}

impl CrashIncident {
    pub fn session_status(&self) -> &'static str {
        match self.kind {
            IncidentKind::Panic => "Previous host session ended during a panic",
            IncidentKind::PlatformCrash => {
                "The operating system recorded a crash in the previous host session"
            }
            IncidentKind::UncleanExit => {
                "Previous KONTRA session ended without enough evidence to call it a crash"
            }
        }
    }

    /// True only when the evidence names this a crash rather than a shutdown we
    /// could not positively confirm.
    ///
    /// A host that exits without unloading the plug-in, or that unloads it and
    /// then dies, leaves the same marker on disk as a crash does. Reporting both
    /// as "crash" made every benign quit indistinguishable from a real fault in
    /// triage, so the two are separated here and stay separated all the way into
    /// the payload's fingerprint namespace.
    fn is_confirmed_crash(&self) -> bool {
        match self.kind {
            IncidentKind::Panic => true,
            IncidentKind::PlatformCrash => {
                !self.host_finished_unload && !host_unloaded_cleanly(&self.events)
            }
            IncidentKind::UncleanExit => false,
        }
    }

    pub(crate) fn auto_reportable(&self) -> bool {
        self.source_schema >= 5 && self.is_confirmed_crash()
    }

    pub(crate) const fn kind_label(&self) -> &'static str {
        match self.kind {
            IncidentKind::Panic => "panic",
            IncidentKind::PlatformCrash => "platform_crash",
            IncidentKind::UncleanExit => "unclean_exit",
        }
    }

    /// The build that wrote this incident's marker. It is not necessarily the
    /// running build: a stale install can recover a marker left by a newer one.
    pub(crate) fn build_id(&self) -> &str {
        &self.build_id
    }

    /// The clustering namespace this incident's fingerprint belongs to.
    ///
    /// Confirmed crashes and merely-unclean exits are counted separately.
    /// Sharing one namespace made a run of benign quits look like one recurring
    /// crash, because the fingerprint material is nearly identical for both.
    pub(crate) fn fingerprint_namespace(&self) -> &'static str {
        if self.is_confirmed_crash() {
            "kontra-crash-v2"
        } else {
            "kontra-unclean-exit-v1"
        }
    }

    pub(crate) fn crash_fingerprint(&self) -> Option<String> {
        if self.source_schema < 5 {
            return None;
        }
        let failure = self.panic.as_ref().map_or_else(
            || self.evidence_signature.clone(),
            |panic| format!("{}|{}", panic.location, panic.message),
        );
        let last_action = if host_unloaded_cleanly(&self.events) {
            String::new()
        } else {
            last_fingerprint_action(&self.events)
        };
        let material = format!(
            "{}|{}|{}|{}|{}|{}",
            self.fingerprint_namespace(),
            self.kind_label(),
            self.host_process.to_ascii_lowercase(),
            self.platform.os_version,
            normalized_failure_signature(&failure),
            last_action,
        );
        Some(blake3::hash(material.as_bytes()).to_hex()[..32].to_owned())
    }

    pub fn preview_diagnostics(&self) -> String {
        self.render_diagnostics(false)
    }

    fn render_diagnostics(&self, complete: bool) -> String {
        let panic = self.panic.as_ref().map_or_else(String::new, |panic| {
            let mut rendered = format!(
                "\nRust panic observed: {} [{}] at {} · {}",
                panic.at, panic.thread, panic.location, panic.message
            );
            if !panic.images.is_empty() {
                rendered.push_str(
                    "\nLoaded images for offline symbolization (load address, ASLR slide, UUID, path):",
                );
                for image in &panic.images {
                    let _ = write!(
                        rendered,
                        "\n0x{:x} {} {} {}",
                        image.load_address, image.slide, image.uuid, image.path
                    );
                }
            }
            rendered
        });
        let user_lines = self
            .events
            .iter()
            .filter_map(|event| (!event.user_line.is_empty()).then_some(event.user_line.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        let events = self
            .events
            .iter()
            .map(DiagnosticRecord::render_diagnostic)
            .collect::<Vec<_>>()
            .join("\n");
        let user_lines = if complete {
            user_lines
        } else {
            bounded_tail_lines(&user_lines, 4_000)
        };
        let events = if complete {
            events
        } else {
            bounded_tail_lines(&events, 16_000)
        };
        // A stale marker from an older install is recovered by whatever build
        // starts next, so the recorded version alone reads as if the running
        // build crashed with the old one's binaries.
        let recorded_version =
            if self.build_id.is_empty() || self.build_id == crate::build_info::BUILD.build_hash {
                self.version.clone()
            } else {
                format!("{} (recorded by a previous install)", self.version)
            };
        let mut output = format!(
            "{}\nIncident ID: {}\nClassification: {}\nVersion that recorded this incident: {}\nBuild ID that recorded this incident: {}\nHost: {}\nPlugin format: {}\nHost process: {}\nOS: {}\nArchitecture: {}\nProcess ID: {}\nStarted: {}\nDetected: {}\nDropped diagnostic events: {}{}",
            self.session_status(),
            self.id,
            self.kind_label(),
            recorded_version,
            if self.build_id.is_empty() {
                "Unavailable"
            } else {
                &self.build_id
            },
            if self.host_name.is_empty() {
                "Unavailable"
            } else {
                &self.host_name
            },
            if self.plugin_api.is_empty() {
                "Unavailable"
            } else {
                &self.plugin_api
            },
            self.host_process,
            self.os,
            self.architecture,
            self.pid,
            self.started_at,
            self.detected_at,
            self.dropped_events,
            panic,
        );
        if complete {
            let first = self.events.front().map_or(0, |event| event.sequence);
            let last = self.events.back().map_or(0, |event| event.sequence);
            let gaps: u64 = self
                .events
                .iter()
                .zip(self.events.iter().skip(1))
                .map(|(a, b)| b.sequence.saturating_sub(a.sequence).saturating_sub(1))
                .sum();
            let _ = write!(
                output,
                "\nPersisted records: {}\nSequence coverage: {first}..{last}\nMissing sequences within coverage: {gaps}",
                self.events.len()
            );
            if let Some(source) = &self.local_journal {
                let _ = write!(
                    output,
                    "\nOriginal local journal: {}\nOriginal journal bytes: {}\nOriginal journal BLAKE3: {}\nValidated journal slots: {}\nJournal slots omitted from this report: {}",
                    source.local_file,
                    source.bytes,
                    source.blake3,
                    source.valid_slots,
                    source.omitted_slots
                );
                if source.omitted_slots != 0 || source.capture_error.is_some() {
                    output.push_str("\nPARTIAL JOURNAL REPORT: startup/recent records are included; the complete original remains local.");
                }
                if let Some(error) = &source.capture_error {
                    let _ = write!(output, "\nJournal capture error: {error}");
                }
            }
        }
        let platform = self.platform.render();
        if !platform.is_empty() {
            let _ = write!(output, "\n\nSystem at incident time:\n{platform}");
        }
        if !user_lines.is_empty() {
            let _ = write!(output, "\n\nWhat KONTRA showed:\n{user_lines}");
        }
        let _ = write!(
            output,
            "\n\nLast persisted activity:\n{}",
            if events.is_empty() {
                "No diagnostic events were persisted"
            } else {
                &events
            }
        );
        output
    }
}

fn normalized_failure_signature(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len().min(256));
    let mut number = false;
    for character in value.chars().take(512) {
        if character.is_ascii_digit() {
            if !number {
                normalized.push('#');
                number = true;
            }
        } else {
            number = false;
            normalized.push(character.to_ascii_lowercase());
        }
        if normalized.len() >= 256 {
            break;
        }
    }
    normalized
}

enum ReporterControl {
    Register(u64, mpsc::SyncSender<bool>),
    Shutdown,
    Submitted(String),
    Resume(String),
    ScanQueue(bool, Option<String>),
}

struct ReporterRuntime {
    control_sender: mpsc::Sender<ReporterControl>,
    worker: std::thread::JoinHandle<()>,
    stopping: Arc<AtomicBool>,
}

#[derive(Default)]
struct SupportWorkers {
    closed: bool,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl SupportWorkers {
    fn spawn(&mut self, name: &str, task: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
        if self.closed {
            return Err(std::io::Error::other("crash session is closing"));
        }
        // Finished work is joined now (it has returned; the join waits out only its
        // thread's exit), so a long session does not keep every handle until unload.
        let (finished, running) = std::mem::take(&mut self.handles)
            .into_iter()
            .partition::<Vec<_>, _>(std::thread::JoinHandle::is_finished);
        self.handles = running;
        join_support_workers(finished);
        self.handles.push(
            std::thread::Builder::new()
                .name(name.to_string())
                .spawn(task)?,
        );
        Ok(())
    }

    fn close_and_take(&mut self) -> Vec<std::thread::JoinHandle<()>> {
        self.closed = true;
        std::mem::take(&mut self.handles)
    }
}

struct CrashReporter {
    runtime: Mutex<Option<ReporterRuntime>>,
    recorder: Mutex<Option<Arc<Recorder>>>,
    pending: Arc<Mutex<Option<CrashIncident>>>,
    support_workers: Mutex<SupportWorkers>,
    ready: Arc<AtomicBool>,
    /// The live session marker, shared with the reporter worker.
    ///
    /// Lifecycle transitions are written here and flushed to disk by whichever
    /// thread makes them. Routing them through the worker and waiting on an
    /// acknowledgement meant a busy GUI thread, an undrained channel or a dead
    /// worker left `Active` on disk for a session that shut down cleanly, and
    /// the next launch recovered it as an incident.
    marker: Arc<Mutex<Option<SessionMarker>>>,
    session_path: Mutex<Option<PathBuf>>,
}

#[derive(Default)]
struct SessionOwnership {
    instances: usize,
    background_leases: usize,
}

static SESSION_OWNERSHIP: Mutex<SessionOwnership> = Mutex::new(SessionOwnership {
    instances: 0,
    background_leases: 0,
});
static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);
static REPORTER: LazyLock<CrashReporter> = LazyLock::new(CrashReporter::new);

pub struct CrashSessionGuard {
    instance_id: u64,
    lifecycle: InstanceLifecycle,
    rt_writer: Option<RtWriter>,
    process_seen: bool,
    frame_clock: u64,
    max_buffer_frames: usize,
    last_channels: usize,
}

pub struct CrashBackgroundLease;

impl CrashSessionGuard {
    pub(super) fn register() -> (Self, bool) {
        let instance_id = NEXT_INSTANCE_ID.fetch_add(1, Ordering::Relaxed);
        let mut ownership = SESSION_OWNERSHIP.lock_unpoisoned();
        REPORTER.support_workers.lock_unpoisoned().closed = false;
        let first_instance = ownership.instances == 0 && ownership.background_leases == 0;
        let mut reporter_ready = false;
        if first_instance {
            let (acknowledge, acknowledged) = mpsc::sync_channel(1);
            if REPORTER.ensure_running()
                && REPORTER.send(ReporterControl::Register(instance_id, acknowledge))
            {
                reporter_ready = acknowledged
                    .recv_timeout(std::time::Duration::from_millis(500))
                    .unwrap_or(false);
            }
        } else {
            REPORTER.persist_instance_lifecycle(
                instance_id,
                InstanceLifecycle::Registered,
                None,
                None,
            );
        }
        ownership.instances = ownership.instances.saturating_add(1);
        drop(ownership);
        (
            Self {
                instance_id,
                lifecycle: InstanceLifecycle::Registered,
                rt_writer: REPORTER.register_rt_lane(instance_id),
                process_seen: false,
                frame_clock: 0,
                max_buffer_frames: 0,
                last_channels: 0,
            },
            reporter_ready,
        )
    }

    pub const fn instance_id(&self) -> u64 {
        self.instance_id
    }

    pub fn mark_initializing(&mut self, plugin_api: impl ToString, host_name: Option<&str>) {
        let plugin_api = plugin_api.to_string();
        super::record_host_identity(&plugin_api, host_name);
        self.set_lifecycle(
            InstanceLifecycle::Initializing,
            host_name.map(str::to_owned),
            Some(plugin_api),
        );
    }

    pub fn mark_initialized(&mut self) {
        self.set_lifecycle(InstanceLifecycle::Active, None, None);
    }

    pub fn mark_deactivating(&mut self) {
        self.set_lifecycle(InstanceLifecycle::Deactivating, None, None);
    }

    pub fn mark_deactivated(&mut self) {
        self.set_lifecycle(InstanceLifecycle::Inactive, None, None);
    }

    fn set_lifecycle(
        &mut self,
        lifecycle: InstanceLifecycle,
        host_name: Option<String>,
        plugin_api: Option<String>,
    ) {
        if self.lifecycle == lifecycle && host_name.is_none() && plugin_api.is_none() {
            return;
        }
        let persisted =
            REPORTER.persist_instance_lifecycle(self.instance_id, lifecycle, host_name, plugin_api);
        if persisted {
            self.lifecycle = lifecycle;
        }
    }

    pub fn background_lease(&self) -> CrashBackgroundLease {
        // Background tasks may outlive the last editor/instance.

        let mut ownership = SESSION_OWNERSHIP.lock_unpoisoned();
        ownership.background_leases = ownership.background_leases.saturating_add(1);
        CrashBackgroundLease
    }

    pub fn record_process_block(&mut self, frames: usize, channels: usize) {
        let event_code = if !self.process_seen {
            derpcat_flight_recorder::rt_code::PROCESS_STARTED
        } else if frames > self.max_buffer_frames || channels != self.last_channels {
            derpcat_flight_recorder::rt_code::BUFFER_SHAPE_CHANGED
        } else {
            self.frame_clock = self
                .frame_clock
                .saturating_add(u64::try_from(frames).unwrap_or(u64::MAX));
            return;
        };
        if let Some(writer) = self.rt_writer.as_mut() {
            writer.try_record(RtEvent {
                event_code,
                phase: 0,
                flags: 0,
                frame_clock: self.frame_clock,
                value_a: i64::try_from(frames).unwrap_or(i64::MAX),
                value_b: i64::try_from(channels).unwrap_or(i64::MAX),
            });
        }
        self.max_buffer_frames = self.max_buffer_frames.max(frames);
        self.last_channels = channels;
        self.process_seen = true;
        self.frame_clock = self
            .frame_clock
            .saturating_add(u64::try_from(frames).unwrap_or(u64::MAX));
    }
}

/// Installs the panic-capture hook for one crash-session owner.
///
impl Drop for CrashSessionGuard {
    fn drop(&mut self) {
        self.set_lifecycle(InstanceLifecycle::Inactive, None, None);
        let mut ownership = SESSION_OWNERSHIP.lock_unpoisoned();
        ownership.instances = ownership.instances.saturating_sub(1);
        if ownership.instances == 0 {
            let workers = REPORTER.support_workers.lock_unpoisoned().close_and_take();
            // Joined without the lock: a worker dropping its background lease, or starting
            // another worker, needs it to finish.
            drop(ownership);
            join_support_workers(workers);
            ownership = SESSION_OWNERSHIP.lock_unpoisoned();
        }
        unregister_if_idle(&ownership);
        drop(ownership);
    }
}

impl Drop for CrashBackgroundLease {
    fn drop(&mut self) {
        let mut ownership = SESSION_OWNERSHIP.lock_unpoisoned();
        ownership.background_leases = ownership.background_leases.saturating_sub(1);
        unregister_if_idle(&ownership);
        drop(ownership);
    }
}

fn unregister_if_idle(ownership: &SessionOwnership) {
    if ownership.instances == 0 && ownership.background_leases == 0 {
        REPORTER.shutdown();
    }
}

impl CrashReporter {
    fn new() -> Self {
        Self {
            runtime: Mutex::new(None),
            recorder: Mutex::new(None),
            // Disk discovery and legacy migration run on the reporter worker.
            pending: Arc::new(Mutex::new(None)),
            support_workers: Mutex::new(SupportWorkers::default()),
            ready: Arc::new(AtomicBool::new(false)),
            marker: Arc::new(Mutex::new(None)),
            session_path: Mutex::new(None),
        }
    }

    fn ensure_running(&self) -> bool {
        let mut runtime = self.runtime.lock_unpoisoned();
        if runtime.is_some() {
            return true;
        }

        let run_token = new_run_token();
        let session_path = current_session_path(&run_token);
        let bootstrap_path = bootstrap_journal_path(&run_token);
        let recorder = Recorder::start(
            bootstrap_path.clone(),
            EVENT_CHANNEL_CAPACITY,
            JOURNAL_BYTES,
        )
        .or_else(|_| {
            Recorder::start(
                std::env::temp_dir().join(format!("kontra-{}-{run_token}.dfr", std::process::id())),
                EVENT_CHANNEL_CAPACITY,
                JOURNAL_BYTES,
            )
        })
        .ok()
        .map(Arc::new);
        *self.session_path.lock_unpoisoned() = Some(session_path.clone());
        let (control_sender, control_receiver) = mpsc::channel();
        let worker_marker = Arc::clone(&self.marker);
        let worker_pending = Arc::clone(&self.pending);
        let worker_ready = Arc::clone(&self.ready);
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = Arc::clone(&stopping);
        let worker_recorder = recorder.as_ref().map(Arc::clone);
        let worker = std::thread::Builder::new()
            .name("kontra-crash-reporter".to_string())
            .spawn(move || {
                reporter_worker(
                    control_receiver,
                    worker_marker,
                    worker_pending,
                    worker_ready,
                    worker_recorder,
                    session_path,
                    bootstrap_path,
                    run_token,
                    worker_stopping,
                );
            });
        let Ok(worker) = worker else {
            if let Some(recorder) = recorder {
                let _ = recorder.shutdown(std::time::Duration::from_millis(500));
            }
            self.ready.store(false, Ordering::Release);
            return false;
        };
        *self.recorder.lock_unpoisoned() = recorder;
        *runtime = Some(ReporterRuntime {
            control_sender,
            worker,
            stopping,
        });
        true
    }

    fn send(&self, control: ReporterControl) -> bool {
        self.runtime
            .lock_unpoisoned()
            .as_ref()
            .is_some_and(|runtime| runtime.control_sender.send(control).is_ok())
    }

    fn register_rt_lane(&self, instance_id: u64) -> Option<RtWriter> {
        self.recorder
            .lock_unpoisoned()
            .as_ref()
            .map(|recorder| recorder.register_rt_lane(instance_id, RT_CHANNEL_CAPACITY))
    }

    fn diagnostic_sink(&self) -> Option<Sink> {
        self.recorder
            .lock_unpoisoned()
            .as_ref()
            .map(|recorder| recorder.sink())
    }

    /// Records an instance lifecycle transition and flushes the marker on the
    /// calling thread.
    ///
    /// Returns whether the marker reached disk. The write is deliberately not
    /// delegated to the reporter worker: a transition that has not been
    /// persisted by the time the process dies is indistinguishable on the next
    /// launch from a session that never made the transition at all.
    fn persist_instance_lifecycle(
        &self,
        instance_id: u64,
        lifecycle: InstanceLifecycle,
        host_name: Option<String>,
        plugin_api: Option<String>,
    ) -> bool {
        let Some(path) = self.session_path.lock_unpoisoned().clone() else {
            return false;
        };
        let mut marker = self.marker.lock_unpoisoned();
        let Some(current) = marker.as_mut() else {
            return false;
        };
        current.apply_lifecycle(instance_id, lifecycle, host_name, plugin_api);
        persist_json(&path, current)
    }

    fn shutdown(&self) {
        let runtime = self.runtime.lock_unpoisoned().take();
        if let Some(runtime) = runtime {
            runtime.stopping.store(true, Ordering::Release);
            let _ = runtime.control_sender.send(ReporterControl::Shutdown);
            // The join can wait for an uninterruptible kernel I/O call, but plugin code must
            // never survive host unload.
            let _ = runtime.worker.join();
        }
        // The worker normally removes the marker in its `Shutdown` arm, but a
        // worker that never started, already exited, or missed the two-second
        // acknowledgement would leave one behind for the next launch to recover
        // as an incident. Removing it here does not depend on any of that.
        let session_path = self.session_path.lock_unpoisoned().take();
        if let Some(path) = session_path {
            let _ = std::fs::remove_file(&path);
        }
        let _ = self.marker.lock_unpoisoned().take();
        let recorder = self.recorder.lock_unpoisoned().take();
        if let Some(recorder) = recorder {
            let _ = recorder.shutdown(std::time::Duration::from_secs(2));
        }
        self.ready.store(false, Ordering::Release);
    }
}

pub fn pending_incident() -> Option<CrashIncident> {
    REPORTER.pending.lock_unpoisoned().clone()
}

/// Starts one-shot work that waits on the user or the network (a file dialog, a licence
/// request, a report, an update check), which unload must never wait for, so nothing joins it.
/// The task must own, or hold `Arc`s to, everything it touches, never the instance, and report
/// through a channel whose receiver may already be gone. The first call pins KONTRA's module, so
/// such a thread never returns into code the host has unmapped.
pub(crate) fn spawn_detached(
    name: &str,
    task: impl FnOnce() + Send + 'static,
) -> std::io::Result<()> {
    pin_own_module().map_err(std::io::Error::other)?;
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(task)
        .map(drop)
}

/// Keeps the module holding KONTRA's code loaded for the life of the process.
fn pin_own_module() -> Result<(), String> {
    pin_module_containing(pin_own_module as *const std::ffi::c_void)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pin_module_containing(address: *const std::ffi::c_void) -> Result<(), String> {
    // SAFETY: `dladdr` only reads the loader's tables; the file name it returns stays valid
    // while the module is loaded, which it is while its address is in use here.
    let path = unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(address, &mut info) == 0 || info.dli_fname.is_null() {
            return Err("dladdr found no module".to_owned());
        }
        // The reference taken here is never released: that is the pin.
        let flags = libc::RTLD_NOW | libc::RTLD_NOLOAD | libc::RTLD_NODELETE;
        if !libc::dlopen(info.dli_fname, flags).is_null() {
            return Ok(());
        }
        std::path::PathBuf::from(std::ffi::OsStr::from_encoded_bytes_unchecked(
            std::ffi::CStr::from_ptr(info.dli_fname).to_bytes(),
        ))
    };
    // The executable itself (a test binary, a standalone build) is never unloaded.
    let canonical = |path: &std::path::Path| path.canonicalize().ok();
    if canonical(&path).is_some()
        && canonical(&path) == std::env::current_exe().ok().and_then(|exe| canonical(&exe))
    {
        return Ok(());
    }
    Err(format!("dlopen refused {}", path.display()))
}

#[cfg(target_os = "windows")]
fn pin_module_containing(address: *const std::ffi::c_void) -> Result<(), String> {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    };
    let mut module = std::ptr::null_mut();
    // SAFETY: with FROM_ADDRESS the name argument is an address inside the module.
    let pinned = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            address.cast(),
            &mut module,
        )
    };
    if pinned == 0 {
        return Err(format!(
            "GetModuleHandleExW: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn pin_module_containing(_address: *const std::ffi::c_void) -> Result<(), String> {
    Ok(())
}

/// Starts the automatic report on its own thread: it asks this worker for a journal snapshot, so
/// it cannot run here. Tracked with the support workers so it is joined before unload.
fn spawn_auto_report() {
    #[cfg(test)]
    if let Some(observer) = TEST_AUTOMATIC_REPORT_OBSERVER.lock_unpoisoned().as_ref() {
        if let Some(incident) = pending_incident() {
            let _ = observer.send(TestAutomaticEvent::Ready(
                incident.id,
                super::report::test_automatic_report_is_busy(),
            ));
        }
        return;
    }
    let _ = REPORTER.support_workers.lock_unpoisoned().spawn(
        "kontra-crash-autoreport",
        super::try_auto_report_pending_incident,
    );
}

fn join_support_workers(workers: Vec<std::thread::JoinHandle<()>>) {
    for worker in workers {
        let _ = worker.join();
    }
}

pub(crate) fn refresh_pending_incident(incident_id: &str) -> Option<CrashIncident> {
    let (incident, observed_hash) = pending_snapshot(incident_id).or_else(|| {
        pending_incident()
            .filter(|i| i.id == incident_id)
            .map(|i| (i, String::new()))
    })?;
    if incident.kind != IncidentKind::UncleanExit || incident.panic.is_some() {
        if let Some(current) = REPORTER
            .pending
            .lock_unpoisoned()
            .as_mut()
            .filter(|i| i.id == incident_id)
        {
            *current = incident.clone();
        }
        return Some(incident);
    }

    let evidence = super::platform::collect_crash_evidence(
        &incident.host_process,
        incident.pid,
        incident.started_at,
        incident.detected_at,
        &AtomicBool::new(false),
    );
    let mut pending = REPORTER.pending.lock_unpoisoned();
    let current = pending
        .as_mut()
        .filter(|current| current.id == incident_id)?;
    let mut refreshed = incident;
    if apply_delayed_crash_evidence(&mut refreshed, &evidence)
        && save_pending_if_unchanged(&refreshed, &observed_hash)
    {
        *current = refreshed;
    } else if let Some(latest) = load_pending_by_id(incident_id) {
        *current = latest;
    }
    Some(current.clone())
}

fn apply_delayed_crash_evidence(
    incident: &mut CrashIncident,
    evidence: &super::platform::CrashEvidence,
) -> bool {
    if incident.kind != IncidentKind::UncleanExit
        || incident.panic.is_some()
        || evidence.disposition != super::platform::EvidenceDisposition::Crash
    {
        return false;
    }
    incident.kind = IncidentKind::PlatformCrash;
    incident.evidence_signature.clone_from(&evidence.signature);
    incident.platform_evidence.clone_from(&evidence.text);
    true
}

pub(crate) fn capture_ready() -> bool {
    REPORTER.ready.load(Ordering::Acquire)
}

pub(crate) fn flush_journal(timeout: std::time::Duration) -> Result<(), String> {
    let recorder = REPORTER.recorder.lock_unpoisoned().clone();
    if recorder
        .as_ref()
        .is_none_or(|recorder| recorder.flush(timeout))
    {
        Ok(())
    } else {
        Err("The crash journal could not flush the latest activity".into())
    }
}

pub fn diagnostic_sink() -> Option<Sink> {
    REPORTER.diagnostic_sink()
}

pub(crate) fn mark_submitted(incident_id: &str) {
    {
        let mut pending = REPORTER.pending.lock_unpoisoned();
        if pending
            .as_ref()
            .is_some_and(|incident| incident.id == incident_id)
        {
            *pending = None;
        }
    }
    let _ = REPORTER.send(ReporterControl::Submitted(incident_id.to_string()));
}

pub(super) fn continue_automatic_reports(incident_id: String) {
    let _ = REPORTER.send(ReporterControl::Resume(incident_id));
}

#[cfg(test)]
static TEST_AUTOMATIC_REPORT_OBSERVER: Mutex<Option<mpsc::Sender<TestAutomaticEvent>>> =
    Mutex::new(None);

#[cfg(test)]
enum TestAutomaticEvent {
    Ready(String, bool),
    Scanned(bool, bool),
}

#[cfg(test)]
pub(crate) fn enrich_diagnostics(incident_id: &str, base: &str) -> String {
    incident_diagnostics(incident_id, base, false)
}

pub(crate) fn complete_diagnostics(incident_id: &str) -> String {
    incident_diagnostics(incident_id, "", true)
}

fn incident_diagnostics(incident_id: &str, base: &str, complete: bool) -> String {
    // The in-memory incident comes first. `save_pending_incident` can fail --
    // the same "Storage write failed" that breaks the tile cache also breaks
    // this -- and reading only from disk then silently dropped every frame,
    // every correlated evidence block and the whole platform section while the
    // payload still went out classified `platform_crash`.
    let Some(incident) = load_pending_by_id(incident_id)
        .or_else(|| pending_incident().filter(|incident| incident.id == incident_id))
    else {
        return base.to_string();
    };
    let preview = incident.preview_diagnostics();
    let current = current_diagnostics_body(base, &preview);
    let current = attributable_current_diagnostics(&incident, current);
    if !complete {
        return assemble_enriched_diagnostics(&preview, &incident.platform_evidence, &current);
    }
    format!(
        "Crash evidence collected with permission:\n{}\n\nCorrelated operating-system or host evidence:\n{}\n\nCurrent KONTRA diagnostics:\n{current}",
        incident.render_diagnostics(true),
        if incident.platform_evidence.is_empty() {
            "No correlated native crash report was collected."
        } else {
            &incident.platform_evidence
        },
    )
}

/// The incident host and the running host must agree before this session's live
/// diagnostics are attached to a recovered incident.
///
/// A marker can outlive its host process and be recovered inside a completely
/// different DAW, which produced reports headed `Host process: reaper.exe`
/// carrying FL Studio's live diagnostics underneath.
fn attributable_current_diagnostics(incident: &CrashIncident, current: &str) -> String {
    if current.trim().is_empty() || incident_host_is_current_process(&incident.host_process) {
        return current.to_owned();
    }
    format!(
        "Withheld: this incident was recorded in {} and recovered in {}. \
Attaching this session's diagnostics would describe the wrong host.",
        display_process_name(&incident.host_process),
        display_process_name(&current_process_name()),
    )
}

fn incident_host_is_current_process(incident_host: &str) -> bool {
    let incident = comparable_process_name(incident_host);
    !incident.is_empty() && incident == comparable_process_name(&current_process_name())
}

fn current_process_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn display_process_name(value: &str) -> String {
    if value.trim().is_empty() {
        "an unknown host".to_owned()
    } else {
        value.to_owned()
    }
}

/// The platform evidence block is the one section that cannot be reconstructed
/// after the fact, so it is budgeted first and the reconstructible sections are
/// squeezed around it. Nothing is elided across the whole assembled body.
fn assemble_enriched_diagnostics(preview: &str, platform_evidence: &str, current: &str) -> String {
    const HEADINGS: usize = 128;
    const EVIDENCE_BUDGET: usize = 12_000;
    const CURRENT_BUDGET: usize = 5_000;

    let platform_evidence = bounded_head_tail(platform_evidence, EVIDENCE_BUDGET);
    let current = bounded_head_tail(current, CURRENT_BUDGET);
    let preview_budget = MAX_EVIDENCE_CHARS
        .saturating_sub(HEADINGS)
        .saturating_sub(platform_evidence.chars().count())
        .saturating_sub(current.chars().count());
    let preview = bounded_head_tail(preview, preview_budget);

    let mut combined = String::with_capacity(MAX_EVIDENCE_CHARS);
    combined.push_str("Crash evidence collected with permission:\n");
    combined.push_str(&preview);
    if !platform_evidence.is_empty() {
        combined.push_str("\n\nCorrelated operating-system or host evidence:\n");
        combined.push_str(&platform_evidence);
    }
    combined.push_str("\n\nCurrent KONTRA diagnostics:\n");
    combined.push_str(&current);
    combined
}

fn current_diagnostics_body<'a>(base: &'a str, preview: &str) -> &'a str {
    base.split_once("\n\nCurrent KONTRA diagnostics:\n")
        .map_or_else(
            || base.strip_prefix(preview).unwrap_or(base),
            |(_, body)| body,
        )
        .trim_start_matches('\n')
}

fn reporter_worker(
    control_receiver: mpsc::Receiver<ReporterControl>,
    marker: Arc<Mutex<Option<SessionMarker>>>,
    pending: Arc<Mutex<Option<CrashIncident>>>,
    ready: Arc<AtomicBool>,
    recorder: Option<Arc<Recorder>>,
    session_path: PathBuf,
    bootstrap_path: PathBuf,
    run_token: String,
    stopping: Arc<AtomicBool>,
) {
    let mut resumable_id: Option<String> = None;
    while let Ok(control) = control_receiver.recv() {
        match control {
            ReporterControl::Register(instance_id, acknowledge) => {
                let mut current = new_session_marker(instance_id, &run_token);
                if let (Some(recorder), Some(journal_file)) =
                    (recorder.as_ref(), current.journal_file.as_ref())
                {
                    if recorder
                        .rotate(
                            sessions_dir().join(journal_file),
                            std::time::Duration::from_millis(500),
                        )
                        .is_err()
                    {
                        current.journal_file = None;
                    } else {
                        let _ = std::fs::remove_file(&bootstrap_path);
                    }
                }
                let persisted = persist_json(&session_path, &current);
                *marker.lock_unpoisoned() = Some(current);
                ready.store(persisted, Ordering::Release);
                let _ = acknowledge.send(persisted);
                super::report::restore_last_report_status();
                // Persist the initialization marker before invoking any platform subprocess.
                // The plugin may open/crash while system metadata is still being collected.
                let platform = super::platform::snapshot(&stopping).clone();
                if let Some(current) = marker.lock_unpoisoned().as_mut() {
                    current.platform = platform;
                    persist_json(&session_path, current);
                }
                // Scanned only after replying: the first instance waits for that reply while
                // holding `SESSION_OWNERSHIP`, and evidence collection can take seconds per stale
                // session. An incident found here reports itself, since the reply has gone.
                // The scan runs without the `pending` lock so the GUI never waits on it.
                let recovered = recover_pending_slot(&pending, &stopping);
                if recovered && persisted {
                    spawn_auto_report();
                }
                if persisted
                    && !pending
                        .lock_unpoisoned()
                        .as_ref()
                        .is_some_and(CrashIncident::auto_reportable)
                {
                    let _ = REPORTER.send(ReporterControl::ScanQueue(false, None));
                }
            }
            ReporterControl::Shutdown => {
                if let Some(recorder) = recorder.as_ref() {
                    let _ = recorder.flush(std::time::Duration::from_millis(500));
                    let _ = recorder.suspend(std::time::Duration::from_millis(500));
                }
                let _ = std::fs::remove_file(&session_path);
                let journal_file = marker
                    .lock_unpoisoned()
                    .take()
                    .and_then(|current| current.journal_file);
                if let Some(journal_file) = journal_file {
                    let _ = std::fs::remove_file(sessions_dir().join(journal_file));
                }
                let _ = std::fs::remove_file(panic_marker_path(std::process::id()));
                break;
            }
            ReporterControl::Submitted(incident_id) => {
                resumable_id = None;
                if incident_id.len() != 16 || !incident_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    continue;
                }
                // Failed/cancelled replacement can leave a same-ID deferred
                // copy. Retire only exact queue copies; full originals stay.
                let mut retired = true;
                for copy in [
                    keyed_pending_incident_path(&incident_id).unwrap(),
                    pending_incident_path(),
                    reports_dir()
                        .join("deferred")
                        .join(format!("{incident_id}.json")),
                ] {
                    if let Err(error) = retire_acknowledged_copy(&copy, &incident_id, &stopping) {
                        if error.kind() != std::io::ErrorKind::NotFound {
                            retired = false;
                            crate::diagnostics::event(
                                crate::diagnostics::LogLevel::Error,
                                "support",
                                "acknowledged_report_cleanup_failed",
                                serde_json::json!({"incident_id":incident_id,
                                "reason":format!("Delivery was acknowledged, but a local queue copy could not be retired ({error}). Evidence remains local and may be retried; complete originals are retained.")}),
                            );
                        }
                    }
                }
                resumable_id = retired.then_some(incident_id);
            }
            ReporterControl::Resume(incident_id) => {
                // Submitted precedes this control in the FIFO. A failed local
                // retirement must never trigger an immediate resend loop.
                if resumable_id.as_deref() == Some(incident_id.as_str())
                    && !stopping.load(Ordering::Acquire)
                {
                    resumable_id = None;
                    let _ = REPORTER.send(ReporterControl::ScanQueue(false, None));
                }
            }
            ReporterControl::ScanQueue(wrapped, scan_cursor) => {
                if stopping.load(Ordering::Acquire)
                    || pending
                        .lock_unpoisoned()
                        .as_ref()
                        .is_some_and(CrashIncident::auto_reportable)
                {
                    continue;
                }
                let (confirmed, _, progress) =
                    find_pending_candidates(&stopping, None, scan_cursor.as_deref());
                #[cfg(test)]
                if let Some(observer) = TEST_AUTOMATIC_REPORT_OBSERVER.lock_unpoisoned().as_ref() {
                    let _ = observer.send(TestAutomaticEvent::Scanned(
                        wrapped,
                        matches!(&progress, QueueScanProgress::Exhausted),
                    ));
                }
                let mut candidate = confirmed.map(|incident| RecoveredCandidate {
                    marker_path: keyed_pending_incident_path(&incident.id).unwrap(),
                    journal_path: None,
                    panic_path: panic_marker_path(incident.pid),
                    incident,
                });
                if matches!(&progress, QueueScanProgress::Exhausted) && wrapped {
                    candidate = candidate
                        .or_else(|| find_deferred_candidate(&stopping))
                        .or_else(|| find_stale_candidate(&stopping, true));
                }
                if let Some(incident) = candidate
                    .and_then(|candidate| candidate.consume(&stopping))
                    .filter(CrashIncident::auto_reportable)
                {
                    *pending.lock_unpoisoned() = Some(incident);
                    spawn_auto_report();
                    continue; // Resume only after this delivery's ACK + permit drop.
                }
                let next = match progress {
                    QueueScanProgress::Advanced(cursor) => Some((wrapped, cursor)),
                    QueueScanProgress::Exhausted if !wrapped => Some((true, String::new())),
                    _ => None,
                };
                if let Some((wrapped, cursor)) = next {
                    // A queued control allows shutdown/registration/ACK to run
                    // between batches. No recursion, timer or delivery retry.
                    let _ = REPORTER.send(ReporterControl::ScanQueue(wrapped, Some(cursor)));
                }
            }
        }
    }
}

fn retire_acknowledged_copy(
    path: &Path,
    incident_id: &str,
    stopping: &AtomicBool,
) -> std::io::Result<()> {
    if stopping.load(Ordering::Acquire) {
        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
    }
    let _publisher_lock =
        buffr_durable_file::acquire_publisher_lock(path, std::time::Duration::from_millis(500))?;
    if stopping.load(Ordering::Acquire) {
        return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
    }
    let Some(bytes) = super::read_bounded_file(path, super::LOCAL_REPORT_BYTES)? else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "queue copy exceeds the validation budget; retained without retirement",
        ));
    };
    let incident: CrashIncident = serde_json::from_slice(&bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "queue copy cannot be validated; retained without retirement",
        )
    })?;
    if incident.id == incident_id {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn detect_stale_sessions(stopping: &AtomicBool) -> Option<CrashIncident> {
    find_stale_candidate(stopping, false)?.consume(stopping)
}

fn find_stale_candidate(stopping: &AtomicBool, confirmed_only: bool) -> Option<RecoveredCandidate> {
    if stopping.load(Ordering::Acquire) {
        return None;
    }
    let directory = sessions_dir();
    let entries = std::fs::read_dir(&directory).ok()?;
    let mut confirmed: Option<RecoveredCandidate> = None;
    let mut unknown: Option<RecoveredCandidate> = None;
    for entry in entries.flatten() {
        if stopping.load(Ordering::Acquire) {
            return None;
        }
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| buffr_durable_file::is_internal_file_name(&name.to_string_lossy()))
        {
            continue; // Live publishers own lock/in-flight files; they are not session JSON.
        }
        if path.extension().is_some_and(|extension| extension == "dfr") {
            continue;
        }
        let Some(mut marker) = read_json_or_discard::<SessionMarker>(&path) else {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > super::LOCAL_REPORT_BYTES as u64) {
                let _ = preserve_original_file(
                    &path,
                    "oversized-session-json-unparsed",
                    stopping,
                    None,
                );
            }
            continue;
        };
        if marker.pid == std::process::id()
            || super::platform::process_is_alive(marker.pid, &marker.host_process)
        {
            continue;
        }
        let journal_path = marker.journal_file.as_deref().and_then(|name| {
            Path::new(name)
                .file_name()
                .map(|file_name| sessions_dir().join(file_name))
        });
        let panic_path = panic_marker_path(marker.pid);
        if marker.schema < 5 {
            cleanup_session_files(&path, journal_path.as_deref(), &panic_path);
            continue;
        }
        let mut local_journal = None;
        if let Some(journal_path) = journal_path.as_ref() {
            let capture = derpcat_flight_recorder::capture_journal(
                journal_path,
                RECOVERED_HEAD_RECORDS,
                RECOVERED_TAIL_RECORDS,
                stopping,
            );
            let records = match capture {
                Ok(capture) => {
                    marker.dropped_events = marker.dropped_events.max(capture.dropped_events);
                    local_journal = Some(LocalJournalEvidence {
                        bytes: capture.bytes,
                        blake3: capture.blake3,
                        valid_slots: capture.valid_slots,
                        omitted_slots: capture.omitted_slots,
                        local_file: format!(
                            "sessions/{}",
                            journal_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ),
                        capture_error: None,
                    });
                    capture.records
                }
                Err(error) => {
                    if stopping.load(Ordering::Acquire) {
                        return None;
                    }
                    local_journal = Some(LocalJournalEvidence {
                        bytes: std::fs::metadata(journal_path).map_or(0, |m| m.len()),
                        local_file: format!(
                            "sessions/{}",
                            journal_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ),
                        capture_error: Some(error.to_string()),
                        ..Default::default()
                    });
                    Vec::new()
                }
            };
            marker.dropped_events = records
                .iter()
                .filter(|record| record.action == "diagnostic_overflow")
                .filter_map(|record| u64::try_from(record.value_a).ok())
                .max()
                .unwrap_or(0)
                .max(marker.dropped_events);
            marker.events = records.into();
        }
        let panic = read_json(&panic_path);
        if panic.is_none()
            && std::fs::metadata(&panic_path)
                .is_ok_and(|m| m.len() > super::LOCAL_REPORT_BYTES as u64)
        {
            let _ = preserve_original_file(
                &panic_path,
                "oversized-panic-json-unparsed",
                stopping,
                None,
            );
        }

        let detected_at = now_unix();
        let started_at = marker.started_at.min(detected_at);
        let evidence = if marker.schema >= 4 || panic.is_some() {
            if stopping.load(Ordering::Acquire) {
                return None;
            }
            super::platform::collect_crash_evidence(
                &marker.host_process,
                marker.pid,
                started_at,
                detected_at,
                stopping,
            )
        } else {
            super::platform::CrashEvidence::default()
        };
        if stopping.load(Ordering::Acquire) {
            return None;
        }
        if panic.is_none()
            && evidence.disposition != super::platform::EvidenceDisposition::Crash
            && session_ended_cleanly(&marker)
        {
            cleanup_session_files(&path, journal_path.as_deref(), &panic_path);
            continue;
        }
        let kind = if panic.is_some() {
            IncidentKind::Panic
        } else if evidence.disposition == super::platform::EvidenceDisposition::Crash {
            IncidentKind::PlatformCrash
        } else {
            IncidentKind::UncleanExit
        };
        let host_finished_unload =
            marker_finished_unload(&marker) || host_unloaded_cleanly(&marker.events);
        let candidate = RecoveredCandidate {
            marker_path: path,
            journal_path,
            panic_path,
            incident: CrashIncident {
                id: incident_id(&marker),
                version: marker.version,
                build_id: marker.build_id,
                started_at,
                detected_at,
                host_process: marker.host_process,
                pid: marker.pid,
                os: marker.os,
                architecture: marker.architecture,
                host_name: marker.host_name,
                plugin_api: marker.plugin_api,
                platform: marker.platform,
                dropped_events: marker.dropped_events,
                panic,
                kind,
                evidence_signature: evidence.signature,
                platform_evidence: evidence.text,
                source_schema: marker.schema,
                host_finished_unload,
                local_journal,
                events: marker.events,
            },
        };
        if candidate.incident.auto_reportable() {
            if confirmed
                .as_ref()
                .is_none_or(|current| candidate.incident.started_at > current.incident.started_at)
            {
                // Other confirmed incidents stay on disk for subsequent recovery.
                confirmed = Some(candidate);
            }
        } else if unknown
            .as_ref()
            .is_none_or(|current| candidate.incident.started_at > current.incident.started_at)
        {
            unknown = Some(candidate);
        }
        // Unselected candidates retain their original marker and journal: native
        // evidence can arrive later and an unclean exit alone is not a crash.
    }
    if stopping.load(Ordering::Acquire) {
        return None;
    }
    confirmed.or_else(|| if confirmed_only { None } else { unknown })
}

fn recover_pending_slot(pending: &Mutex<Option<CrashIncident>>, stopping: &AtomicBool) -> bool {
    let previous = pending.lock_unpoisoned().clone();
    if previous
        .as_ref()
        .is_some_and(CrashIncident::auto_reportable)
    {
        return false;
    }
    // Migration failure retains the legacy source, but cannot block independent
    // keyed reports: no recovered incident writes that shared legacy path.
    let _ = migrate_legacy_pending(stopping);
    let (queued, unknown, _) =
        find_pending_candidates(stopping, previous.as_ref().map(|i| i.id.as_str()), None);
    let queued_candidate = |incident: CrashIncident| RecoveredCandidate {
        marker_path: keyed_pending_incident_path(&incident.id).unwrap(),
        journal_path: None,
        panic_path: panic_marker_path(incident.pid),
        incident,
    };
    let candidate = queued
        .map(queued_candidate)
        .or_else(|| find_deferred_candidate(stopping))
        .or_else(|| find_stale_candidate(stopping, previous.is_some()))
        .or_else(|| {
            if previous.is_none() {
                unknown.map(queued_candidate)
            } else {
                None
            }
        });
    let Some(candidate) = candidate else {
        return false;
    };
    // Reserve the slot after the expensive scan. A delayed-evidence worker may
    // have confirmed the previous incident meanwhile; never replace that report.
    {
        let mut slot = pending.lock_unpoisoned();
        if slot.as_ref().map(|i| &i.id) != previous.as_ref().map(|i| &i.id)
            || slot.as_ref().is_some_and(CrashIncident::auto_reportable)
        {
            return false;
        }
        *slot = None;
    }
    if let Some(previous) = previous.as_ref() {
        if previous.id.len() != 16 || !previous.id.bytes().all(|b| b.is_ascii_hexdigit()) {
            *pending.lock_unpoisoned() = Some(previous.clone());
            return false;
        }
        if !persist_json(
            &reports_dir()
                .join("deferred")
                .join(format!("{}.json", previous.id)),
            previous,
        ) {
            *pending.lock_unpoisoned() = Some(previous.clone());
            return false;
        }
        crate::diagnostics::event(
            crate::diagnostics::LogLevel::Warning,
            "support",
            "unconfirmed_evidence_deferred",
            serde_json::json!({"incident_id":previous.id,
            "reason":"Earlier unconfirmed session evidence remains private in local deferred storage; a confirmed crash can now report. Deferred records are checked again for late native confirmation."}),
        );
    }
    let recovered = candidate.consume(stopping);
    let mut slot = pending.lock_unpoisoned();
    match recovered {
        Some(incident) => {
            *slot = Some(incident);
            true
        }
        None => {
            *slot = previous;
            false
        }
    }
}

// Recheck at most 32 deferred records per registration, rotating the durable
// cursor across launches. Originals survive every unsuccessful native lookup.
fn find_deferred_candidate(stopping: &AtomicBool) -> Option<RecoveredCandidate> {
    let directory = reports_dir().join("deferred");
    let entries = std::fs::read_dir(&directory).ok()?;
    let cursor_path = reports_dir().join("deferred-cursor.json");
    let cursor = read_json::<usize>(&cursor_path).unwrap_or(0);
    let mut seen = 0;
    let mut checked = 0;
    for entry in entries.flatten() {
        if stopping.load(Ordering::Acquire) {
            return None;
        }
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        seen += 1;
        if seen <= cursor {
            continue;
        }
        checked += 1;
        let incident: Option<CrashIncident> = read_json(&path);
        if incident.is_none()
            && std::fs::metadata(&path).is_ok_and(|m| m.len() > super::LOCAL_REPORT_BYTES as u64)
        {
            let _ =
                preserve_original_file(&path, "oversized-deferred-json-unparsed", stopping, None);
        }
        if let Some(mut incident) = incident {
            if !incident.auto_reportable() {
                let evidence = super::platform::collect_crash_evidence(
                    &incident.host_process,
                    incident.pid,
                    incident.started_at,
                    incident.detected_at,
                    stopping,
                );
                apply_delayed_crash_evidence(&mut incident, &evidence);
            }
            if incident.auto_reportable() && !stopping.load(Ordering::Acquire) {
                persist_json(&cursor_path, &seen);
                return Some(RecoveredCandidate {
                    marker_path: path,
                    journal_path: None,
                    panic_path: panic_marker_path(incident.pid),
                    incident,
                });
            }
        }
        if checked == 32 {
            persist_json(&cursor_path, &seen);
            return None;
        }
    }
    persist_json(&cursor_path, &0_usize);
    None
}

struct RecoveredCandidate {
    marker_path: PathBuf,
    journal_path: Option<PathBuf>,
    panic_path: PathBuf,
    incident: CrashIncident,
}

impl RecoveredCandidate {
    fn consume(self, stopping: &AtomicBool) -> Option<CrashIncident> {
        let Self {
            marker_path,
            journal_path,
            panic_path,
            mut incident,
        } = self;
        if keyed_pending_incident_path(&incident.id).as_ref() == Some(&marker_path) {
            let _publisher_lock = buffr_durable_file::acquire_publisher_lock(
                &marker_path,
                std::time::Duration::from_millis(500),
            )
            .ok()?;
            if stopping.load(Ordering::Acquire) {
                return None;
            }
            // Another host may have refreshed this ID since discovery. Adopt
            // its latest durable bytes; never write the observed snapshot back.
            return read_json::<CrashIncident>(&marker_path)
                .and_then(normalize_pending)
                .filter(|current| current.id == incident.id);
        }
        let mut archived = journal_path.is_none();
        if let Some(journal) = journal_path.as_ref() {
            let name = format!("{}.dfr", incident.id);
            let archive = reports_dir().join("originals").join(&name);
            if let Err(error) =
                archive_original(journal, &archive, incident.local_journal.as_ref(), stopping)
            {
                if let Some(source) = incident.local_journal.as_mut() {
                    source.capture_error = Some(format!(
                        "Original archival failed: {error}. The session source remains local; it may differ from the captured digest."
                    ));
                }
                crate::diagnostics::event(
                    crate::diagnostics::LogLevel::Warning,
                    "support",
                    "original_journal_archive_failed",
                    serde_json::json!({"reason":format!("Original journal remains in local session storage; archival failed: {error}")}),
                );
            } else {
                archived = true;
                if let Some(source) = incident.local_journal.as_mut() {
                    source.local_file = format!("originals/{name}");
                }
            }
        }
        if stopping.load(Ordering::Acquire) {
            return None;
        }
        // Keep the original files if the recovered incident cannot be saved.
        if !save_pending_incident(&incident) {
            return None;
        }
        if keyed_pending_incident_path(&incident.id).as_ref() != Some(&marker_path) {
            cleanup_session_files(
                &marker_path,
                if archived {
                    journal_path.as_deref()
                } else {
                    None
                },
                &panic_path,
            );
        }
        load_pending_by_id(&incident.id).or(Some(incident))
    }
}

fn archive_original(
    source: &Path,
    archive: &Path,
    expected: Option<&LocalJournalEvidence>,
    stopping: &AtomicBool,
) -> std::io::Result<()> {
    use std::io::{Read as _, Write as _};
    let mut original = std::fs::File::open(source)?;
    buffr_durable_file::publish_private_streaming(
        archive,
        std::time::Duration::from_millis(500),
        |destination| {
            let mut buffer = [0_u8; 64 * 1024];
            let mut hasher = blake3::Hasher::new();
            let mut total = 0_u64;
            loop {
                if stopping.load(Ordering::Acquire) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "original archival stopped",
                    ));
                }
                let bytes = original.read(&mut buffer)?;
                if bytes == 0 {
                    if let Some(expected) = expected.filter(|e| !e.blake3.is_empty()) {
                        if total != expected.bytes
                            || hasher.finalize().to_hex().as_str() != expected.blake3.as_str()
                        {
                            return Err(std::io::Error::other(
                                "original journal changed after capture; session source retained",
                            ));
                        }
                    }
                    return Ok(());
                }
                hasher.update(&buffer[..bytes]);
                total += bytes as u64;
                destination.write_all(&buffer[..bytes])?;
            }
        },
    )
}

/// Preserve complete source bytes privately without buffering the full file.
/// A second streamed pass validates the captured digest before atomic publication.
pub(super) fn preserve_original_file(
    source: &Path,
    status: &str,
    stopping: &AtomicBool,
    parsed_original: Option<(u64, &str)>,
) -> std::io::Result<String> {
    let result = (|| {
        use std::io::Read as _;
        let mut file = std::fs::File::open(source)?;
        let mut buffer = [0_u8; 64 * 1024];
        let mut bytes = 0_u64;
        let mut hasher = blake3::Hasher::new();
        loop {
            if stopping.load(Ordering::Acquire) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes += read as u64;
            hasher.update(&buffer[..read]);
        }
        let hash = hasher.finalize().to_hex().to_string();
        if parsed_original
            .is_some_and(|(parsed_bytes, parsed_hash)| parsed_bytes != bytes || parsed_hash != hash)
        {
            return Err(std::io::Error::other(
                "Native source changed after parsing; a different file was not archived as the parsed report",
            ));
        }
        let relative = format!("originals/{hash}.raw");
        let archive = reports_dir().join(&relative);
        let expected = LocalJournalEvidence {
            bytes,
            blake3: hash.clone(),
            ..Default::default()
        };
        archive_original(source, &archive, Some(&expected), stopping)?;
        let manifest = serde_json::json!({"status":status, "bytes":bytes, "blake3":hash,
        "local_file":relative, "buffered_read_limit":super::LOCAL_REPORT_BYTES,
        "automatically_uploaded":false});
        if !persist_json(&archive.with_extension("json"), &manifest) {
            return Err(std::io::Error::other(
                "Complete original was archived, but its private manifest could not be published; source retained",
            ));
        }
        Ok(format!(
            "Complete original retained privately: {relative}; bytes={bytes}; BLAKE3={hash}; status={status}; raw original is retained locally and not uploaded as a file."
        ))
    })();
    let (level, reason) = match &result {
        Ok(status) => (crate::diagnostics::LogLevel::Info, status.clone()),
        Err(error) => (
            crate::diagnostics::LogLevel::Error,
            format!("Private original archival failed: {error}. KONTRA did not remove the source."),
        ),
    };
    crate::diagnostics::event(
        level,
        "support",
        "complete_original_retention",
        serde_json::json!({"reason":reason,"status":status}),
    );
    result
}

fn preserve_oversized_pending(stopping: &AtomicBool) -> bool {
    let path = pending_incident_path();
    if stopping.load(Ordering::Acquire) {
        return false;
    }
    let _publisher_lock = match buffr_durable_file::acquire_publisher_lock(
        &path,
        std::time::Duration::from_millis(500),
    ) {
        Ok(lock) => lock,
        Err(error) => {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "support",
                "pending_report_lock_failed",
                serde_json::json!({"reason":format!(
                    "Pending evidence could not be locked for safe recovery ({error}); replacement remains blocked.")}),
            );
            return false;
        }
    };
    if stopping.load(Ordering::Acquire) {
        return false;
    }
    match super::read_bounded_file(&path, super::LOCAL_REPORT_BYTES) {
        Ok(Some(_)) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Ok(None) => {
            match preserve_original_file(&path, "oversized-pending-json-unparsed", stopping, None) {
                Ok(status) => {
                    crate::diagnostics::event(
                        crate::diagnostics::LogLevel::Warning,
                        "support",
                        "oversized_pending_report_archived",
                        serde_json::json!({"reason":format!("{status} It could not be parsed or automatically submitted; export the private original for support.")}),
                    );
                    if stopping.load(Ordering::Acquire) {
                        return false;
                    }
                    match std::fs::remove_file(path) {
                        Ok(()) => true,
                        Err(error) => {
                            crate::diagnostics::event(
                                crate::diagnostics::LogLevel::Error,
                                "support",
                                "oversized_pending_slot_retirement_failed",
                                serde_json::json!({"reason":format!("The complete oversized original is privately archived, but its pending slot could not be retired ({error}); replacement remains blocked.")}),
                            );
                            false
                        }
                    }
                }
                Err(error) => {
                    crate::diagnostics::event(
                        crate::diagnostics::LogLevel::Error,
                        "support",
                        "oversized_pending_report_archive_failed",
                        serde_json::json!({"reason":format!("Oversized pending evidence remains in place and blocks replacement because private archival failed: {error}")}),
                    );
                    false
                }
            }
        }
        Err(error) => {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "support",
                "pending_report_read_failed",
                serde_json::json!({"reason":format!("Pending evidence could not be read and remains in place; replacement is blocked: {error}")}),
            );
            false
        }
    }
}

fn cleanup_session_files(marker: &Path, journal: Option<&Path>, panic: &Path) {
    let _ = std::fs::remove_file(marker);
    if let Some(journal) = journal {
        let _ = std::fs::remove_file(journal);
    }
    let _ = std::fs::remove_file(panic);
}

#[cfg(test)]
fn evidence_tail(
    records: Vec<derpcat_flight_recorder::Record>,
) -> VecDeque<derpcat_flight_recorder::Record> {
    let mut evidence = records
        .into_iter()
        .filter(derpcat_flight_recorder::Record::is_evidence)
        .collect::<Vec<_>>();
    if evidence.len() > MAX_DIAGNOSTIC_EVENTS {
        evidence.drain(..evidence.len() - MAX_DIAGNOSTIC_EVENTS);
    }
    evidence.into()
}

fn marker_finished_unload(marker: &SessionMarker) -> bool {
    marker.schema >= 5
        && !marker.instances.is_empty()
        && marker
            .instances
            .values()
            .all(|lifecycle| matches!(lifecycle, InstanceLifecycle::Inactive))
}

fn host_unloaded_cleanly(events: &VecDeque<DiagnosticRecord>) -> bool {
    journal_ended_cleanly(events)
        && events.iter().any(|event| {
            event.subsystem == derpcat_flight_recorder::Subsystem::Host
                && event.action == "plugin_deactivate"
                && event.phase == derpcat_flight_recorder::Phase::Completed
        })
}

fn last_fingerprint_action(events: &VecDeque<DiagnosticRecord>) -> String {
    events
        .iter()
        .rev()
        .filter(|event| event.is_evidence())
        .find(|event| {
            !(event.action == "plugin_deactivate"
                && event.phase == derpcat_flight_recorder::Phase::Completed)
        })
        .map(derpcat_flight_recorder::Record::fingerprint_action)
        .unwrap_or_default()
}

fn session_ended_cleanly(marker: &SessionMarker) -> bool {
    if marker.schema >= 5
        && !marker.instances.is_empty()
        && marker
            .instances
            .values()
            .all(|lifecycle| !lifecycle.is_unclean_if_process_dies())
    {
        return true;
    }
    if marker.schema == 4 && marker.active_instances.is_empty() {
        return true;
    }
    journal_ended_cleanly(&marker.events)
}

fn journal_ended_cleanly(events: &VecDeque<DiagnosticRecord>) -> bool {
    let mut instances = BTreeMap::<u64, InstanceLifecycle>::new();
    for event in events {
        if event.subsystem != derpcat_flight_recorder::Subsystem::Host {
            continue;
        }
        let instance_id = event.instance_id.unwrap_or(0);
        let lifecycle = match (event.action.as_str(), event.phase) {
            ("plugin_initialize", derpcat_flight_recorder::Phase::Started) => {
                Some(InstanceLifecycle::Initializing)
            }
            ("plugin_initialize", derpcat_flight_recorder::Phase::Completed) => {
                Some(InstanceLifecycle::Active)
            }
            ("plugin_initialize", derpcat_flight_recorder::Phase::Failed)
            | ("plugin_deactivate", derpcat_flight_recorder::Phase::Completed) => {
                Some(InstanceLifecycle::Inactive)
            }
            ("plugin_deactivate", derpcat_flight_recorder::Phase::Started) => {
                Some(InstanceLifecycle::Deactivating)
            }
            _ => None,
        };
        if let Some(lifecycle) = lifecycle {
            instances.insert(instance_id, lifecycle);
        }
    }
    !instances.is_empty()
        && instances
            .values()
            .all(|lifecycle| !lifecycle.is_unclean_if_process_dies())
}

fn new_session_marker(instance_id: u64, run_token: &str) -> SessionMarker {
    let pid = std::process::id();
    let started_at = now_unix();
    let host_process = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Unavailable".to_string());
    let session_id = blake3::hash(
        format!(
            "{}:{pid}:{started_at}:{host_process}:{run_token}",
            env!("CARGO_PKG_VERSION")
        )
        .as_bytes(),
    )
    .to_hex()[..16]
        .to_string();
    let identity = super::HOST_IDENTITY.lock_unpoisoned().clone();
    SessionMarker {
        schema: 5,
        journal_file: Some(format!("{session_id}.dfr")),
        session_id,
        pid,
        version: env!("CARGO_PKG_VERSION").to_string(),
        build_id: crate::build_info::BUILD.build_hash.to_string(),
        started_at,
        host_process,
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        host_name: identity.0,
        plugin_api: identity.1,
        platform: super::platform::PlatformSnapshot {
            os_name: std::env::consts::OS.into(),
            process_architecture: std::env::consts::ARCH.into(),
            ..Default::default()
        },
        events: VecDeque::new(),
        dropped_events: 0,
        active_instances: BTreeSet::new(),
        instances: BTreeMap::from([(instance_id, InstanceLifecycle::Initializing)]),
    }
}

fn incident_id(marker: &SessionMarker) -> String {
    blake3::hash(marker.session_id.as_bytes()).to_hex()[..16].to_string()
}

fn reports_dir() -> PathBuf {
    let base = super::support_cache_path();
    base.parent().map_or_else(
        || std::env::temp_dir().join("kontra-crash-reports"),
        |parent| parent.join("crash-reports"),
    )
}

fn sessions_dir() -> PathBuf {
    reports_dir().join("sessions")
}

fn new_run_token() -> String {
    let mut random = [0_u8; 16];
    let _ = getrandom::fill(&mut random);
    blake3::hash(
        format!(
            "{}:{}:{:?}:{:x}:{random:?}",
            std::process::id(),
            now_unix(),
            std::thread::current().id(),
            new_run_token as *const () as usize,
        )
        .as_bytes(),
    )
    .to_hex()[..16]
        .to_string()
}

fn current_session_path(run_token: &str) -> PathBuf {
    sessions_dir().join(format!("{}-{run_token}.json", std::process::id()))
}

fn bootstrap_journal_path(run_token: &str) -> PathBuf {
    reports_dir().join(format!("bootstrap-{}-{run_token}.dfr", std::process::id()))
}

fn panic_marker_path(pid: u32) -> PathBuf {
    reports_dir().join("panics").join(format!("{pid}.json"))
}

fn pending_incident_path() -> PathBuf {
    reports_dir().join("pending.json")
}

fn keyed_pending_incident_path(incident_id: &str) -> Option<PathBuf> {
    (incident_id.len() == 16 && incident_id.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
        reports_dir()
            .join("pending")
            .join(format!("{incident_id}.json"))
    })
}

fn normalize_pending(mut incident: CrashIncident) -> Option<CrashIncident> {
    if incident.source_schema < 5 || keyed_pending_incident_path(&incident.id).is_none() {
        return None;
    }
    incident.detected_at = incident.detected_at.max(incident.started_at);
    if incident.panic.is_some() {
        incident.kind = IncidentKind::Panic;
    } else if incident.kind == IncidentKind::UncleanExit && journal_ended_cleanly(&incident.events)
    {
        return None;
    }
    Some(incident)
}

fn pending_snapshot(incident_id: &str) -> Option<(CrashIncident, String)> {
    let path = keyed_pending_incident_path(incident_id)?;
    let bytes = super::read_bounded_file(&path, super::LOCAL_REPORT_BYTES).ok()??;
    let incident = normalize_pending(serde_json::from_slice::<CrashIncident>(&bytes).ok()?)?;
    (incident.id == incident_id).then(|| (incident, blake3::hash(&bytes).to_hex().to_string()))
}

fn load_pending_by_id(incident_id: &str) -> Option<CrashIncident> {
    pending_snapshot(incident_id).map(|(incident, _)| incident)
}

fn save_pending_if_unchanged(incident: &CrashIncident, expected_hash: &str) -> bool {
    let Some(path) = keyed_pending_incident_path(&incident.id) else {
        return false;
    };
    let Ok(bytes) = serde_json::to_vec(incident) else {
        return false;
    };
    buffr_durable_file::publish_private_streaming(
        &path,
        std::time::Duration::from_millis(500),
        |file| {
            let Some(current) = super::read_bounded_file(&path, super::LOCAL_REPORT_BYTES)? else {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
            };
            if blake3::hash(&current).to_hex().as_str() != expected_hash {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "incident changed during evidence collection",
                ));
            }
            let previous: CrashIncident =
                serde_json::from_slice(&current).map_err(std::io::Error::other)?;
            if !same_pending_identity(&previous, incident)
                || (previous.auto_reportable() && !incident.auto_reportable())
            {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
            }
            preserve_original_file(
                &path,
                "keyed-pending-before-proof-refresh",
                &AtomicBool::new(false),
                Some((current.len() as u64, expected_hash)),
            )?;
            std::io::Write::write_all(file, &bytes)
        },
    )
    .is_ok()
}

/// Add stronger proof without replacing the current host/build/journal fields.
/// A matching ID alone cannot authorize joining evidence from another identity.
fn same_pending_identity(current: &CrashIncident, incoming: &CrashIncident) -> bool {
    current.id == incoming.id
        && current.pid == incoming.pid
        && current.started_at == incoming.started_at
        && comparable_process_name(&current.host_process)
            == comparable_process_name(&incoming.host_process)
        && current.version == incoming.version
        && current.build_id == incoming.build_id
        && current.plugin_api == incoming.plugin_api
        && current.os == incoming.os
        && current.architecture == incoming.architecture
}

fn stronger_pending_proof(
    current: &CrashIncident,
    incoming: &CrashIncident,
) -> Option<CrashIncident> {
    if current.auto_reportable()
        || !incoming.auto_reportable()
        || !same_pending_identity(current, incoming)
    {
        return None;
    }
    let mut merged = current.clone();
    merged.kind = incoming.kind;
    merged.panic = incoming.panic.clone();
    merged.evidence_signature = incoming.evidence_signature.clone();
    merged.platform_evidence = incoming.platform_evidence.clone();
    merged.detected_at = merged.detected_at.max(incoming.detected_at);
    merged.dropped_events = merged.dropped_events.max(incoming.dropped_events);
    Some(merged)
}

fn save_pending_incident(incident: &CrashIncident) -> bool {
    let Some(path) = keyed_pending_incident_path(&incident.id) else {
        return false;
    };
    let Ok(bytes) = serde_json::to_vec(incident) else {
        return false;
    };
    // Each incident owns its filename. Different hosts cannot replace each
    // other's queue slots; only same-ID evidence refreshes use this lock.
    match buffr_durable_file::publish_private_streaming(
        &path,
        std::time::Duration::from_millis(500),
        |file| match super::read_bounded_file(&path, super::LOCAL_REPORT_BYTES) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::io::Write::write_all(file, &bytes)
            }
            Ok(Some(existing)) => {
                let current: CrashIncident =
                    serde_json::from_slice(&existing).map_err(std::io::Error::other)?;
                if current.source_schema < 5 || !same_pending_identity(&current, incident) {
                    return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
                }
                if let Some(merged) = stronger_pending_proof(&current, incident) {
                    let digest = blake3::hash(&existing).to_hex().to_string();
                    preserve_original_file(
                        &path,
                        "keyed-pending-before-stronger-proof",
                        &AtomicBool::new(false),
                        Some((existing.len() as u64, &digest)),
                    )?;
                    let merged = serde_json::to_vec(&merged).map_err(std::io::Error::other)?;
                    std::io::Write::write_all(file, &merged)
                } else {
                    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
                }
            }
            _ => Err(std::io::Error::from(std::io::ErrorKind::InvalidData)),
        },
    ) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => true,
        Err(error) => {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "support",
                "pending_report_write_failed",
                serde_json::json!({"incident_id":incident.id,
                "reason":format!("Incident could not be durably queued; existing/source evidence was not retired: {error}")}),
            );
            false
        }
    }
}

enum QueueScanProgress {
    Advanced(String),
    Exhausted,
    Halted,
}

/// Inspect at most 32 safe queue files per registration. A lexical cursor
/// advances through large queues without keeping all filenames in memory.
/// Select the oldest confirmed incident in this batch before any unknown one.
fn find_pending_candidates(
    stopping: &AtomicBool,
    excluded_id: Option<&str>,
    scan_cursor: Option<&str>,
) -> (
    Option<CrashIncident>,
    Option<CrashIncident>,
    QueueScanProgress,
) {
    let directory = reports_dir().join("pending");
    let cursor_path = reports_dir().join("pending-cursor.json");
    // A draining pass carries its own monotonic cursor: other hosts may update
    // the persisted registration cursor, but cannot make this pass revisit a batch.
    let cursor = scan_cursor
        .map(str::to_owned)
        .unwrap_or_else(|| read_json::<String>(&cursor_path).unwrap_or_default());
    let mut paths = BTreeMap::new();
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (None, None, QueueScanProgress::Exhausted);
        }
        Err(_) => return (None, None, QueueScanProgress::Halted),
    };
    for entry in entries.flatten() {
        if stopping.load(Ordering::Acquire) {
            return (None, None, QueueScanProgress::Halted);
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if name <= cursor
            || keyed_pending_incident_path(id).is_none()
            || !entry.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        paths.insert(name, entry.path());
        if paths.len() > 32 {
            paths.pop_last();
        }
    }
    let mut confirmed: Option<CrashIncident> = None;
    let mut unknown: Option<CrashIncident> = None;
    for (name, path) in &paths {
        if stopping.load(Ordering::Acquire) {
            return (None, None, QueueScanProgress::Halted);
        }
        let Some((mut incident, observed_hash)) =
            name.strip_suffix(".json").and_then(pending_snapshot)
        else {
            if std::fs::metadata(path).is_ok_and(|m| m.len() > super::LOCAL_REPORT_BYTES as u64) {
                let _ = preserve_original_file(
                    path,
                    "oversized-keyed-pending-json-unparsed",
                    stopping,
                    None,
                );
            }
            continue;
        };
        if Some(incident.id.as_str()) == excluded_id || format!("{}.json", incident.id) != *name {
            continue;
        }
        if !incident.auto_reportable() {
            let evidence = super::platform::collect_crash_evidence(
                &incident.host_process,
                incident.pid,
                incident.started_at,
                incident.detected_at,
                stopping,
            );
            if apply_delayed_crash_evidence(&mut incident, &evidence)
                && !save_pending_if_unchanged(&incident, &observed_hash)
            {
                let Some(latest) = load_pending_by_id(&incident.id) else {
                    continue;
                };
                incident = latest; // Never overwrite another host's fresher evidence.
            }
        }
        let selection = if incident.auto_reportable() {
            &mut confirmed
        } else {
            &mut unknown
        };
        if selection.as_ref().is_none_or(|old| {
            (incident.started_at, incident.detected_at, &incident.id)
                < (old.started_at, old.detected_at, &old.id)
        }) {
            *selection = Some(incident);
        }
    }
    let next = paths
        .last_key_value()
        .map(|(name, _)| name.as_str())
        .unwrap_or("");
    let progress = if stopping.load(Ordering::Acquire) || !persist_json(&cursor_path, &next) {
        QueueScanProgress::Halted
    } else if paths.is_empty() {
        QueueScanProgress::Exhausted
    } else {
        QueueScanProgress::Advanced(next.to_owned())
    };
    (confirmed, unknown, progress)
}

/// Migrate only on a worker. Hold the legacy publisher lock from exact read
/// through complete private raw+manifest retention, keyed publication and
/// retirement. Any failure/cancellation leaves legacy evidence in place.
fn migrate_legacy_pending(stopping: &AtomicBool) -> bool {
    if stopping.load(Ordering::Acquire) {
        return false;
    }
    let path = pending_incident_path();
    let lock = match buffr_durable_file::acquire_publisher_lock(
        &path,
        std::time::Duration::from_millis(500),
    ) {
        Ok(lock) => lock,
        Err(error) => {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "support",
                "pending_migration_failed",
                serde_json::json!({"reason":format!("Legacy pending evidence remains local; migration could not lock it: {error}")}),
            );
            return false;
        }
    };
    let bytes = match super::read_bounded_file(&path, super::LOCAL_REPORT_BYTES) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            drop(lock);
            return preserve_oversized_pending(stopping);
        }
        Err(error) => {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "support",
                "pending_migration_failed",
                serde_json::json!({"reason":format!("Legacy pending evidence could not be read; it remains local and migration is blocked: {error}")}),
            );
            return false;
        }
    };
    let incident = serde_json::from_slice::<CrashIncident>(&bytes)
        .ok()
        .and_then(normalize_pending);
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let result = (|| -> std::io::Result<()> {
        preserve_original_file(
            &path,
            "legacy-pending-json-migration",
            stopping,
            Some((bytes.len() as u64, &hash)),
        )?;
        if let Some(incident) = incident {
            let destination = keyed_pending_incident_path(&incident.id).unwrap();
            let serialized = serde_json::to_vec(&incident).map_err(std::io::Error::other)?;
            let publication = buffr_durable_file::publish_private_streaming(
                &destination,
                std::time::Duration::from_millis(500),
                |file| match super::read_bounded_file(&destination, super::LOCAL_REPORT_BYTES) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        std::io::Write::write_all(file, &serialized)
                    }
                    Ok(Some(existing)) => {
                        let current: CrashIncident =
                            serde_json::from_slice(&existing).map_err(std::io::Error::other)?;
                        if current.source_schema < 5 || !same_pending_identity(&current, &incident)
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "legacy/keyed incident identity differs; legacy original retained",
                            ));
                        }
                        if let Some(merged) = stronger_pending_proof(&current, &incident) {
                            let digest = blake3::hash(&existing).to_hex().to_string();
                            preserve_original_file(
                                &destination,
                                "keyed-pending-before-legacy-proof",
                                stopping,
                                Some((existing.len() as u64, &digest)),
                            )?;
                            let merged =
                                serde_json::to_vec(&merged).map_err(std::io::Error::other)?;
                            std::io::Write::write_all(file, &merged)
                        } else {
                            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
                        }
                    }
                    _ => Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "keyed destination cannot be validated; legacy original retained",
                    )),
                },
            );
            if let Err(error) = publication {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
        }
        if stopping.load(Ordering::Acquire) {
            return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
        }
        std::fs::remove_file(&path)
    })();
    match result {
        Ok(()) => true,
        Err(error) => {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Error,
                "support",
                "pending_migration_failed",
                serde_json::json!({"reason":format!("Legacy pending evidence was not retired because complete private retention/keyed migration failed: {error}")}),
            );
            false
        }
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = super::read_bounded_file(path, super::LOCAL_REPORT_BYTES).ok()??;
    serde_json::from_slice(&bytes).ok()
}

/// Discards malformed bounded session JSON only while holding its publisher lock.
/// Oversized originals remain for the recovery worker's private archival.
fn read_json_or_discard<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let _publisher_lock =
        buffr_durable_file::acquire_publisher_lock(path, std::time::Duration::from_millis(500))
            .ok()?;
    let bytes = super::read_bounded_file(path, super::LOCAL_REPORT_BYTES).ok()??;
    let value = serde_json::from_slice(&bytes).ok();
    if value.is_none() {
        let _ = std::fs::remove_file(path);
    }
    value
}

/// Saves the panic marker, or writes its evidence to `fallback` (stderr, which
/// hosts usually log) when the disk refuses it, so a full disk does not lose
/// the backtrace.
#[cfg(test)]
fn persist_panic_marker(path: &Path, marker: &PanicMarker, fallback: &mut impl std::io::Write) {
    if !persist_json(path, marker) {
        let _ = fallback.write_all(
            format!(
                "KONTRA could not save panic evidence to {}; panicked at {} on {}:\n{}\n",
                path.display(),
                marker.location,
                marker.thread,
                marker.message
            )
            .as_bytes(),
        );
    }
}

fn persist_json(path: &Path, value: &impl Serialize) -> bool {
    let Ok(bytes) = serde_json::to_vec(value) else {
        return false;
    };
    match buffr_durable_file::publish_private_streaming(
        path,
        std::time::Duration::from_millis(500),
        |file| std::io::Write::write_all(file, &bytes),
    ) {
        Ok(()) => true,
        Err(error) => {
            eprintln!(
                "KONTRA could not persist crash evidence at {}: {error}",
                path.display()
            );
            false
        }
    }
}

fn bounded_tail_lines(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    let tail = value
        .chars()
        .skip(count.saturating_sub(max_chars))
        .collect::<String>();
    tail.split_once('\n')
        .map(|(_, complete_lines)| complete_lines.to_owned())
        .unwrap_or(tail)
}

fn bounded_head_tail(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_owned();
    }
    let marker = "\n...[middle omitted to keep the newest evidence]...\n";
    let available = max_chars.saturating_sub(marker.chars().count());
    let head_chars = available * 2 / 3;
    let tail_chars = available.saturating_sub(head_chars);
    let head = value.chars().take(head_chars).collect::<String>();
    let tail = value
        .chars()
        .skip(count.saturating_sub(tail_chars))
        .collect::<String>();
    format!("{head}{marker}{tail}")
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::super::platform::{CrashEvidence, EvidenceDisposition};
    use super::*;
    use derpcat_flight_recorder::{Importance, Phase, Record, Subsystem};

    fn host_record(action: &str, phase: Phase) -> Record {
        Record {
            sequence: 1,
            subsystem: Subsystem::Host,
            action: action.to_string(),
            phase,
            instance_id: Some(1),
            importance: Importance::Evidence,
            ..Record::default()
        }
    }

    fn incident(kind: IncidentKind, events: Vec<Record>) -> CrashIncident {
        CrashIncident {
            kind,
            events: events.into(),
            ..test_incident("0.8.45", "build")
        }
    }

    fn panicked(images: Vec<LoadedImage>) -> CrashIncident {
        let mut incident = incident(IncidentKind::Panic, vec![]);
        incident.panic = Some(PanicMarker {
            at: 1,
            thread: "gui".to_string(),
            message: "boom".to_string(),
            location: "src/editor.rs:1".to_string(),
            images,
        });
        incident
    }

    fn evidence_state(incident: &CrashIncident) -> (IncidentKind, &str, &str) {
        let signature = incident.evidence_signature.as_str();
        (
            incident.kind,
            signature,
            incident.platform_evidence.as_str(),
        )
    }

    /// Late evidence upgrades only an unclean exit, and the refreshed preview is not duplicated.

    #[test]
    fn interleaved_instance_unload_preserves_the_other_active_instance() {
        let mut marker = new_session_marker(1, "test");
        marker.apply_lifecycle(1, InstanceLifecycle::Active, None, None);
        marker.apply_lifecycle(2, InstanceLifecycle::Active, None, None);
        marker.apply_lifecycle(1, InstanceLifecycle::Inactive, None, None);
        assert!(!session_ended_cleanly(&marker));
        marker.apply_lifecycle(2, InstanceLifecycle::Deactivating, None, None);
        assert!(!session_ended_cleanly(&marker));
        marker.apply_lifecycle(2, InstanceLifecycle::Inactive, None, None);
        assert!(session_ended_cleanly(&marker));
    }

    #[test]
    fn recovered_incident_survives_retry_and_is_consumed_once() {
        const CHILD: &str = "KONTRA_RECOVERY_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "support::crash::tests::recovered_incident_survives_retry_and_is_consumed_once",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let mut marker = new_session_marker(1, "recovered");
        marker.pid = u32::MAX - 7;
        marker.host_process = "authored-test-host".into();
        let marker_path = sessions_dir().join("fixture.json");
        let journal = sessions_dir().join(marker.journal_file.as_ref().unwrap());
        let recorder = Recorder::start(journal.clone(), 32, 0).unwrap();
        recorder.sink().record(
            Subsystem::Host,
            "plugin_initialize",
            Phase::Started,
            derpcat_flight_recorder::Context {
                instance_id: Some(1),
                ..Default::default()
            },
            "authored startup",
        );
        assert!(recorder.flush(std::time::Duration::from_secs(2)));
        // Exceed the recovered window through the actual recorder/consumer, without
        // queue overflow. Complete originals must survive the bounded report view.
        for sequence in 0..(RECOVERED_HEAD_RECORDS + RECOVERED_TAIL_RECORDS + 17) {
            recorder.sink().record(
                Subsystem::Host,
                "fixture_event",
                Phase::Started,
                Default::default(),
                format!("authored event {sequence}"),
            );
            if sequence % 16 == 0 {
                assert!(recorder.flush(std::time::Duration::from_secs(2)));
            }
        }
        assert!(recorder.flush(std::time::Duration::from_secs(2)));
        assert!(recorder.shutdown(std::time::Duration::from_secs(2)));
        // Put the sole persisted overflow count in the omitted middle. Rewrite
        // an authored slot with the real recorder schema/checksum so the root
        // consumer (not only the vendor reader test) must retain the aggregate.
        let middle = derpcat_flight_recorder::Record {
            sequence: 140,
            action: "diagnostic_overflow".into(),
            value_a: 87,
            ..Default::default()
        };
        let payload = serde_json::to_vec(&middle).unwrap();
        assert!(payload.len() <= 1024 - 52);
        let mut slot = [0_u8; 1024];
        slot[..8].copy_from_slice(b"DFRSLT01");
        slot[8..10].copy_from_slice(&1_u16.to_le_bytes());
        slot[10..12].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        slot[12..20].copy_from_slice(&middle.sequence.to_le_bytes());
        slot[20..52].copy_from_slice(blake3::hash(&payload).as_bytes());
        slot[52..52 + payload.len()].copy_from_slice(&payload);
        {
            use std::io::{Seek as _, Write as _};
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&journal)
                .unwrap();
            file.seek(std::io::SeekFrom::Start(140 * 1024)).unwrap();
            file.write_all(&slot).unwrap();
            file.sync_all().unwrap();
        }
        let original_journal = std::fs::read(&journal).unwrap();
        assert!(persist_json(&marker_path, &marker));
        let panic = PanicMarker {
            at: now_unix(),
            thread: "test".into(),
            message: "owned failure and backtrace frame".into(),
            location: "src/fixture.rs:9:2".into(),
            images: vec![],
        };
        assert!(persist_json(&panic_marker_path(marker.pid), &panic));
        let live_path = sessions_dir().join("live-protected.json");
        let live_marker = new_session_marker(99, "live-protected");
        assert!(persist_json(&live_path, &live_marker));
        let live_lock = buffr_durable_file::lock_path(&live_path).unwrap();
        let in_flight = sessions_dir().join(format!(
            ".live-protected.json.tmp-{}-42",
            std::process::id()
        ));
        std::fs::write(&in_flight, b"in-flight authored fixture").unwrap();
        assert!(live_lock.exists());
        let recovered = detect_stale_sessions(&AtomicBool::new(false)).unwrap();
        assert!(
            live_path.exists() && live_lock.exists() && in_flight.exists(),
            "recovery must not discard a live publisher's lock or temporary"
        );
        assert!(recovered.auto_reportable());
        assert!(
            recovered
                .render_diagnostics(true)
                .contains("authored startup")
        );
        assert!(
            recovered
                .render_diagnostics(true)
                .contains("src/fixture.rs:9:2")
        );
        assert!(!marker_path.exists() && !journal.exists());
        let source = recovered.local_journal.as_ref().unwrap();
        assert!(source.omitted_slots > 0);
        assert_eq!(recovered.dropped_events, 87);
        assert!(
            !recovered
                .events
                .iter()
                .any(|r| r.action == "diagnostic_overflow")
        );
        assert_eq!(
            recovered.events.len(),
            RECOVERED_HEAD_RECORDS + RECOVERED_TAIL_RECORDS
        );
        assert_eq!(source.bytes, original_journal.len() as u64);
        assert_eq!(
            source.blake3,
            blake3::hash(&original_journal).to_hex().to_string()
        );
        let archive = reports_dir().join(&source.local_file);
        assert_eq!(std::fs::read(&archive).unwrap(), original_journal);
        assert!(
            recovered
                .render_diagnostics(true)
                .contains("PARTIAL JOURNAL REPORT")
        );
        assert!(
            recovered
                .render_diagnostics(true)
                .contains("authored event 2192")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(load_pending_by_id(&recovered.id).unwrap().id, recovered.id);
        assert!(
            detect_stale_sessions(&AtomicBool::new(false)).is_none(),
            "consumed marker must not create duplicate reports"
        );
        let pending_before =
            std::fs::read(keyed_pending_incident_path(&recovered.id).unwrap()).unwrap();
        assert!(super::super::request_agent().is_err());
        assert_eq!(
            std::fs::read(keyed_pending_incident_path(&recovered.id).unwrap()).unwrap(),
            pending_before,
            "failed delivery must retain complete evidence"
        );

        // A second crash while the first is offline must not overwrite the first report.
        let mut second = new_session_marker(2, "second-crash");
        second.pid = u32::MAX - 8;
        second.host_process = "second-authored-host".into();
        let second_marker = sessions_dir().join("second-fixture.json");
        let second_journal = sessions_dir().join(second.journal_file.as_ref().unwrap());
        let recorder = Recorder::start(second_journal.clone(), 32, 0).unwrap();
        recorder.sink().record(
            Subsystem::Host,
            "plugin_initialize",
            Phase::Started,
            Default::default(),
            "distinct second crash startup",
        );
        assert!(recorder.shutdown(std::time::Duration::from_secs(2)));
        assert!(persist_json(&second_marker, &second));
        assert!(persist_json(&panic_marker_path(second.pid), &panic));
        let second_marker_before = std::fs::read(&second_marker).unwrap();
        let second_journal_before = std::fs::read(&second_journal).unwrap();

        let (sender, receiver) = mpsc::channel();
        let session = sessions_dir().join("live-retry-session.json");
        let pending = Arc::new(Mutex::new(load_pending_by_id(&recovered.id)));
        let worker = std::thread::spawn(move || {
            reporter_worker(
                receiver,
                Arc::new(Mutex::new(None)),
                pending,
                Arc::new(AtomicBool::new(false)),
                None,
                session,
                bootstrap_journal_path("live-retry"),
                "live-retry".into(),
                Arc::new(AtomicBool::new(false)),
            )
        });
        let (acknowledge, acknowledged) = mpsc::sync_channel(1);
        sender
            .send(ReporterControl::Register(3, acknowledge))
            .unwrap();
        assert!(
            acknowledged
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        );
        sender
            .send(ReporterControl::Submitted("wrong-id".into()))
            .unwrap();
        sender.send(ReporterControl::Shutdown).unwrap();
        worker.join().unwrap();
        assert_eq!(
            std::fs::read(keyed_pending_incident_path(&recovered.id).unwrap()).unwrap(),
            pending_before
        );
        assert_eq!(std::fs::read(&second_marker).unwrap(), second_marker_before);
        assert_eq!(
            std::fs::read(&second_journal).unwrap(),
            second_journal_before
        );

        // An acknowledged first report permits recovery on the next plugin session.
        // Exercise the same Submitted handler used after a verified server receipt.
        let (sender, receiver) = mpsc::channel();
        sender
            .send(ReporterControl::Submitted(recovered.id.clone()))
            .unwrap();
        drop(sender);
        reporter_worker(
            receiver,
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
            None,
            sessions_dir().join("unused.json"),
            bootstrap_journal_path("unused"),
            "unused".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(!keyed_pending_incident_path(&recovered.id).unwrap().exists());
        let second_recovered = detect_stale_sessions(&AtomicBool::new(false)).unwrap();
        assert_ne!(second_recovered.id, recovered.id);
        assert!(
            second_recovered
                .render_diagnostics(true)
                .contains("distinct second crash startup")
        );
        assert_eq!(
            load_pending_by_id(&second_recovered.id).unwrap().id,
            second_recovered.id
        );
        assert!(!second_marker.exists() && !second_journal.exists());
        let second_pending =
            std::fs::read(keyed_pending_incident_path(&second_recovered.id).unwrap()).unwrap();
        assert!(super::super::request_agent().is_err());
        assert_eq!(
            std::fs::read(keyed_pending_incident_path(&second_recovered.id).unwrap()).unwrap(),
            second_pending
        );
        assert!(detect_stale_sessions(&AtomicBool::new(false)).is_none());
    }
    #[test]
    fn unconfirmed_pending_does_not_block_confirmed_recovery_or_lose_deferred_evidence() {
        const CHILD: &str = "KONTRA_DEFERRED_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::unconfirmed_pending_does_not_block_confirmed_recovery_or_lose_deferred_evidence", "--test-threads=1"])
                .env(CHILD,"1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK","1").status().unwrap();
            assert!(status.success());
            return;
        }
        let mut previous = test_incident("0.3.115", "authored-old");
        previous.id = "0123456789abcdef".into();
        previous.kind = IncidentKind::UncleanExit;
        previous.panic = None;
        previous.host_process = "authored-unknown-host".into();
        previous.pid = u32::MAX - 100;
        assert!(save_pending_incident(&previous));
        let previous_bytes =
            std::fs::read(keyed_pending_incident_path(&previous.id).unwrap()).unwrap();
        let mut marker = new_session_marker(12, "later-proven-crash");
        marker.pid = u32::MAX - 101;
        marker.host_process = "authored-proven-host".into();
        marker.journal_file = None;
        let marker_path = sessions_dir().join("later-confirmed.json");
        assert!(persist_json(&marker_path, &marker));
        let panic = PanicMarker {
            at: now_unix(),
            thread: "authored".into(),
            message: "authored failure".into(),
            location: "src/authored.rs:9".into(),
            images: vec![],
        };
        assert!(persist_json(&panic_marker_path(marker.pid), &panic));
        let mut unknown = marker.clone();
        unknown.pid = u32::MAX - 102;
        unknown.session_id = "other-retained-unknown".into();
        let unknown_path = sessions_dir().join("other-unknown.json");
        assert!(persist_json(&unknown_path, &unknown));
        let unknown_bytes = std::fs::read(&unknown_path).unwrap();
        let pending = Arc::new(Mutex::new(Some(previous.clone())));
        let (sender, receiver) = mpsc::channel();
        let (reply, acknowledged) = mpsc::sync_channel(1);
        sender.send(ReporterControl::Register(99, reply)).unwrap();
        sender.send(ReporterControl::Shutdown).unwrap();
        reporter_worker(
            receiver,
            Arc::new(Mutex::new(None)),
            pending.clone(),
            Arc::new(AtomicBool::new(false)),
            None,
            sessions_dir().join("current.json"),
            bootstrap_journal_path("deferred-test"),
            "deferred-test".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(
            acknowledged
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        );
        let recovered = pending.lock_unpoisoned().clone().unwrap();
        assert_eq!(recovered.id, incident_id(&marker));
        assert!(recovered.auto_reportable());
        assert!(!marker_path.exists());
        let deferred = reports_dir()
            .join("deferred")
            .join(format!("{}.json", previous.id));
        assert_eq!(std::fs::read(&deferred).unwrap(), previous_bytes);
        assert_eq!(std::fs::read(&unknown_path).unwrap(), unknown_bytes);
        // Confirmed offline reports still own the slot, even with other candidates.
        let confirmed_bytes =
            std::fs::read(keyed_pending_incident_path(&recovered.id).unwrap()).unwrap();
        assert!(!recover_pending_slot(&pending, &AtomicBool::new(false)));
        assert_eq!(
            std::fs::read(keyed_pending_incident_path(&recovered.id).unwrap()).unwrap(),
            confirmed_bytes
        );
        assert_eq!(std::fs::read(&deferred).unwrap(), previous_bytes);
        // A saved, subsequently confirmed deferred fixture uses the same queue
        // consumer. No OS crash, network request or synthetic public issue is made.
        let evidence = CrashEvidence {
            disposition: EvidenceDisposition::Crash,
            signature: "authored-late-native-proof".into(),
            text: "authored exception and frames".into(),
        };
        assert!(apply_delayed_crash_evidence(&mut previous, &evidence));
        assert!(persist_json(&deferred, &previous));
        retire_acknowledged_copy(
            &keyed_pending_incident_path(&recovered.id).unwrap(),
            &recovered.id,
            &AtomicBool::new(false),
        )
        .unwrap();
        *pending.lock_unpoisoned() = None;
        assert!(recover_pending_slot(&pending, &AtomicBool::new(false)));
        assert_eq!(pending.lock_unpoisoned().as_ref().unwrap().id, previous.id);
        assert!(!deferred.exists());
        assert!(
            unknown_path.exists(),
            "unselected unknown evidence remains available for later native matching"
        );
    }

    #[test]
    fn failed_pending_replacement_then_acknowledgement_does_not_requeue_deferred_duplicate() {
        const CHILD: &str = "KONTRA_FAILED_REPLACEMENT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::failed_pending_replacement_then_acknowledgement_does_not_requeue_deferred_duplicate", "--test-threads=1"])
                .env(CHILD,"1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK","1").status().unwrap();
            assert!(status.success());
            return;
        }
        let mut previous = test_incident("0.3.115", "authored-failed-replacement");
        previous.id = "0123456789abcdef".into();
        previous.panic = None;
        previous.kind = IncidentKind::UncleanExit;
        assert!(save_pending_incident(&previous));
        let prior = reports_dir().join("prior-held.json");
        std::fs::rename(keyed_pending_incident_path(&previous.id).unwrap(), &prior).unwrap();
        // A directory at the publication destination makes the real durable
        // replace fail after the previous incident has been deferred.
        let mut marker = new_session_marker(12, "next-confirmed");
        marker.pid = u32::MAX - 111;
        marker.host_process = "authored-host".into();
        marker.journal_file = None;
        let next_path = keyed_pending_incident_path(&incident_id(&marker)).unwrap();
        std::fs::create_dir(&next_path).unwrap();
        let marker_path = sessions_dir().join("next-confirmed.json");
        assert!(persist_json(&marker_path, &marker));
        assert!(persist_json(
            &panic_marker_path(marker.pid),
            &PanicMarker {
                at: now_unix(),
                thread: "test".into(),
                message: "authored failure".into(),
                location: "src/authored.rs:1".into(),
                images: vec![]
            }
        ));
        let pending = Arc::new(Mutex::new(Some(previous.clone())));
        assert!(!recover_pending_slot(&pending, &AtomicBool::new(false)));
        let deferred = reports_dir()
            .join("deferred")
            .join(format!("{}.json", previous.id));
        assert_eq!(
            read_json::<CrashIncident>(&deferred).unwrap().id,
            previous.id
        );
        assert_eq!(pending.lock_unpoisoned().as_ref().unwrap().id, previous.id);
        assert!(
            marker_path.exists(),
            "failed consume retains next incident source"
        );
        std::fs::remove_dir(&next_path).unwrap();
        std::fs::rename(&prior, keyed_pending_incident_path(&previous.id).unwrap()).unwrap();
        let unrelated = reports_dir().join("deferred").join("fedcba9876543210.json");
        let unrelated_bytes = b"authored unrelated local evidence";
        std::fs::write(&unrelated, unrelated_bytes).unwrap();
        let original = reports_dir().join("originals").join("complete-private.dfr");
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::write(&original, b"full original remains local").unwrap();
        assert!(apply_delayed_crash_evidence(
            &mut previous,
            &CrashEvidence {
                disposition: EvidenceDisposition::Crash,
                signature: "authored-late-proof".into(),
                text: "authored native exception and stack".into()
            }
        ));
        assert!(save_pending_incident(&previous));
        let (sender, receiver) = mpsc::channel();
        sender
            .send(ReporterControl::Submitted(previous.id.clone()))
            .unwrap();
        drop(sender);
        reporter_worker(
            receiver,
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
            None,
            sessions_dir().join("unused.json"),
            bootstrap_journal_path("unused"),
            "unused".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(!keyed_pending_incident_path(&previous.id).unwrap().exists() && !deferred.exists());
        assert_eq!(std::fs::read(&unrelated).unwrap(), unrelated_bytes);
        assert_eq!(
            std::fs::read(&original).unwrap(),
            b"full original remains local"
        );
        *pending.lock_unpoisoned() = None;
        assert!(recover_pending_slot(&pending, &AtomicBool::new(false)));
        assert_eq!(
            pending.lock_unpoisoned().as_ref().unwrap().id,
            incident_id(&marker)
        );
        assert!(
            !deferred.exists(),
            "acknowledged incident never returns to the queue"
        );
    }

    #[test]
    fn oversized_pending_is_not_discarded_or_replaced_before_complete_private_retention() {
        const CHILD: &str = "KONTRA_OVERSIZED_PENDING_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","support::crash::tests::oversized_pending_is_not_discarded_or_replaced_before_complete_private_retention","--test-threads=1"])
                .env(CHILD,"1").env("KONTRA_REPORT_DIR",directory.path()).env("KONTRA_DISABLE_NETWORK","1").status().unwrap();
            assert!(status.success());
            return;
        }
        let mut incident = test_incident("0.3.115", "authored-large-prior");
        incident.platform_evidence =
            "authored private original\n".repeat(super::super::LOCAL_REPORT_BYTES / 20);
        assert!(persist_json(&pending_incident_path(), &incident));
        let original = std::fs::read(pending_incident_path()).unwrap();
        assert!(original.len() > super::super::LOCAL_REPORT_BYTES);
        assert!(read_json::<CrashIncident>(&pending_incident_path()).is_none());
        assert_eq!(
            std::fs::read(pending_incident_path()).unwrap(),
            original,
            "oversize is not malformed JSON and must never be discarded"
        );
        let blocker = reports_dir().join("originals");
        std::fs::write(&blocker, b"authored archive failure blocker").unwrap();
        let pending = Mutex::new(None);
        assert!(!recover_pending_slot(&pending, &AtomicBool::new(false)));
        assert_eq!(std::fs::read(pending_incident_path()).unwrap(), original);
        assert!(pending.lock_unpoisoned().is_none());
        std::fs::remove_file(&blocker).unwrap();
        assert!(!preserve_oversized_pending(&AtomicBool::new(true)));
        assert!(pending_incident_path().exists());
        assert!(preserve_oversized_pending(&AtomicBool::new(false)));
        assert!(!pending_incident_path().exists());
        let hash = blake3::hash(&original).to_hex().to_string();
        let archive = blocker.join(format!("{hash}.raw"));
        assert_eq!(std::fs::read(&archive).unwrap(), original);
        let manifest: serde_json::Value = read_json(&archive.with_extension("json")).unwrap();
        assert_eq!(manifest["bytes"], original.len());
        assert_eq!(manifest["blake3"], hash);
        assert_eq!(manifest["status"], "oversized-pending-json-unparsed");
        assert_eq!(manifest["automatically_uploaded"], false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(archive.with_extension("json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn pending_retirement_serializes_with_an_actual_competing_publisher() {
        const CHILD: &str = "KONTRA_PENDING_RETIREMENT_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::pending_retirement_serializes_with_an_actual_competing_publisher", "--test-threads=1"])
                .env(CHILD, "1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1").status().unwrap();
            assert!(status.success());
            return;
        }
        let path = pending_incident_path();
        let original = vec![b' '; super::super::LOCAL_REPORT_BYTES + 1];
        buffr_durable_file::publish_private(&path, &original).unwrap();
        let hash = blake3::hash(&original).to_hex().to_string();
        let archive = reports_dir().join("originals").join(format!("{hash}.raw"));
        // Hold the archive publisher so the real recovery worker pauses after
        // acquiring the pending publisher lock, before it can retire anything.
        let archive_lock =
            buffr_durable_file::acquire_publisher_lock(&archive, std::time::Duration::ZERO)
                .unwrap();
        let recovery = std::thread::spawn(|| preserve_oversized_pending(&AtomicBool::new(false)));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match buffr_durable_file::acquire_publisher_lock(&path, std::time::Duration::ZERO) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Ok(lock) => drop(lock),
                Err(error) => panic!("unexpected pending lock error: {error}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "recovery did not acquire its lock"
            );
            std::thread::yield_now();
        }
        let mut next = test_incident("0.3.115", "authored-competing-publisher");
        next.id = "fedcba9876543210".into();
        let next_bytes = serde_json::to_vec(&next).unwrap();
        let publisher_path = path.clone();
        let published_bytes = next_bytes.clone();
        let (entered, receiver) = mpsc::channel();
        let publisher = std::thread::spawn(move || {
            buffr_durable_file::publish_private_streaming(
                &publisher_path,
                std::time::Duration::from_secs(2),
                |file| {
                    entered.send(()).unwrap();
                    std::io::Write::write_all(file, &published_bytes)
                },
            )
        });
        assert!(
            matches!(
                receiver.recv_timeout(std::time::Duration::from_millis(25)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "publisher entered while recovery held its lock"
        );
        drop(archive_lock);
        assert!(recovery.join().unwrap());
        publisher.join().unwrap().unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            next_bytes,
            "retirement must not delete the other host's new publication"
        );
        assert_eq!(std::fs::read(&archive).unwrap(), original);
        let stopped = AtomicBool::new(false);
        retire_acknowledged_copy(&path, "0123456789abcdef", &stopped).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            next_bytes,
            "another ID cannot be retired"
        );
        // Busy ACK retirement gives up without deleting, using the same lock as
        // the real publisher. Cancellation also retains the queue copy.
        let lock =
            buffr_durable_file::acquire_publisher_lock(&path, std::time::Duration::ZERO).unwrap();
        assert!(retire_acknowledged_copy(&path, &next.id, &stopped).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), next_bytes);
        drop(lock);
        assert!(retire_acknowledged_copy(&path, &next.id, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), next_bytes);
        retire_acknowledged_copy(&path, &next.id, &stopped).unwrap();
        assert!(!path.exists());
        assert_eq!(
            std::fs::read(&archive).unwrap(),
            original,
            "ACK preserves full original evidence"
        );
    }

    #[test]
    fn two_host_publishers_keep_both_keyed_reports_and_exact_ack_recovers_the_other() {
        const CHILD: &str = "KONTRA_KEYED_QUEUE_CHILD";
        const PUBLISH: &str = "KONTRA_KEYED_QUEUE_PUBLISH_ID";
        fn fixture(id: &str, start: u64) -> CrashIncident {
            let mut incident = test_incident("0.3.115", "authored-keyed-host");
            incident.id = id.into();
            incident.started_at = start;
            incident.detected_at = start + 1;
            incident.pid = u32::MAX - start as u32;
            incident.host_process = format!("authored-host-{start}");
            incident.kind = IncidentKind::PlatformCrash;
            incident.platform_evidence = format!("authored confirmed exception and stack for {id}");
            incident
        }
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::two_host_publishers_keep_both_keyed_reports_and_exact_ack_recovers_the_other", "--test-threads=1"])
                .env(CHILD, "1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1").status().unwrap();
            assert!(status.success());
            return;
        }
        if let Ok(id) = std::env::var(PUBLISH) {
            let start = if id == "0000000000000001" { 100 } else { 200 };
            assert!(save_pending_incident(&fixture(&id, start)));
            return;
        }
        let first = fixture("0000000000000001", 100);
        let second = fixture("0000000000000002", 200);
        let first_bytes = serde_json::to_vec(&first).unwrap();
        let second_bytes = serde_json::to_vec(&second).unwrap();
        let mut publishers = Vec::new();
        for id in [&first.id, &second.id] {
            publishers.push(std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::two_host_publishers_keep_both_keyed_reports_and_exact_ack_recovers_the_other", "--test-threads=1"])
                .env(CHILD, "1").env(PUBLISH, id).env("KONTRA_DISABLE_NETWORK", "1")
                .spawn().unwrap());
        }
        for mut publisher in publishers {
            assert!(publisher.wait().unwrap().success());
        }
        let first_path = keyed_pending_incident_path(&first.id).unwrap();
        let second_path = keyed_pending_incident_path(&second.id).unwrap();
        assert_eq!(std::fs::read(&first_path).unwrap(), first_bytes);
        assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        assert!(
            !pending_incident_path().exists(),
            "new hosts never publish the shared legacy slot"
        );
        let mut older_unknown = fixture("0000000000000003", 50);
        older_unknown.kind = IncidentKind::UncleanExit;
        older_unknown.evidence_signature.clear();
        older_unknown.platform_evidence.clear();
        assert!(save_pending_incident(&older_unknown));
        let unknown_path = keyed_pending_incident_path(&older_unknown.id).unwrap();
        let unknown_bytes = std::fs::read(&unknown_path).unwrap();
        let pending = Arc::new(Mutex::new(None));
        let stopping = AtomicBool::new(false);
        assert!(recover_pending_slot(&pending, &stopping));
        assert_eq!(
            pending.lock_unpoisoned().as_ref().unwrap().id,
            first.id,
            "oldest confirmed in the bounded batch precedes older unknown evidence"
        );
        assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        assert!(keyed_pending_incident_path("../unsafe").is_none());
        let mut invalid = first.clone();
        invalid.id = "../unsafe".into();
        assert!(!save_pending_incident(&invalid));
        // The real Submitted control retires exactly one acknowledged ID.
        let (sender, receiver) = mpsc::channel();
        sender
            .send(ReporterControl::Submitted(first.id.clone()))
            .unwrap();
        drop(sender);
        reporter_worker(
            receiver,
            Arc::new(Mutex::new(None)),
            pending.clone(),
            Arc::new(AtomicBool::new(false)),
            None,
            sessions_dir().join("unused.json"),
            bootstrap_journal_path("unused"),
            "unused".into(),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(!first_path.exists());
        assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        assert_eq!(std::fs::read(&unknown_path).unwrap(), unknown_bytes);
        *pending.lock_unpoisoned() = None; // Same RAM clear performed by verified mark_submitted.
        // The previous cursor reached this batch's last key; the next pass resets
        // it, and the subsequent registration revisits the remaining queue.
        if !recover_pending_slot(&pending, &stopping) {
            assert!(recover_pending_slot(&pending, &stopping));
        }
        assert_eq!(pending.lock_unpoisoned().as_ref().unwrap().id, second.id);
        assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        assert_eq!(std::fs::read(&unknown_path).unwrap(), unknown_bytes);
    }

    #[test]
    fn legacy_migration_retains_exact_original_until_copy_and_manifest_are_durable() {
        const CHILD: &str = "KONTRA_LEGACY_MIGRATION_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::legacy_migration_retains_exact_original_until_copy_and_manifest_are_durable", "--test-threads=1"])
                .env(CHILD, "1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1").status().unwrap();
            assert!(status.success());
            return;
        }
        let mut incident = test_incident("0.3.115", "authored-legacy-migration");
        incident.id = "0123456789abcdef".into();
        incident.kind = IncidentKind::PlatformCrash;
        incident.platform_evidence = "authored legacy exception and all source frames".into();
        let bytes = serde_json::to_vec_pretty(&incident).unwrap();
        buffr_durable_file::publish_private(&pending_incident_path(), &bytes).unwrap();
        let destination = keyed_pending_incident_path(&incident.id).unwrap();
        let originals = reports_dir().join("originals");
        std::fs::write(&originals, b"authored archive failure").unwrap();
        assert!(!migrate_legacy_pending(&AtomicBool::new(false)));
        assert_eq!(std::fs::read(pending_incident_path()).unwrap(), bytes);
        assert!(!destination.exists());
        std::fs::remove_file(&originals).unwrap();
        assert!(!migrate_legacy_pending(&AtomicBool::new(true)));
        assert_eq!(std::fs::read(pending_incident_path()).unwrap(), bytes);
        std::fs::create_dir_all(&destination).unwrap();
        assert!(
            !migrate_legacy_pending(&AtomicBool::new(false)),
            "failed keyed publication must retain legacy"
        );
        assert_eq!(std::fs::read(pending_incident_path()).unwrap(), bytes);
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let archive = originals.join(format!("{hash}.raw"));
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        let manifest: serde_json::Value = read_json(&archive.with_extension("json")).unwrap();
        assert_eq!(manifest["bytes"], bytes.len());
        assert_eq!(manifest["blake3"], hash);
        assert_eq!(manifest["status"], "legacy-pending-json-migration");
        assert_eq!(manifest["automatically_uploaded"], false);
        std::fs::remove_dir(&destination).unwrap();
        assert!(migrate_legacy_pending(&AtomicBool::new(false)));
        assert!(!pending_incident_path().exists());
        assert_eq!(
            load_pending_by_id(&incident.id).unwrap().platform_evidence,
            incident.platform_evidence
        );
        // An already refreshed keyed copy must never be replaced by older legacy
        // evidence; its complete older original still survives privately.
        let mut updated = incident.clone();
        updated.platform_evidence.push_str(" + later proof");
        let observed = pending_snapshot(&incident.id).unwrap().1;
        assert!(save_pending_if_unchanged(&updated, &observed));
        let updated_bytes = std::fs::read(&destination).unwrap();
        buffr_durable_file::publish_private(&pending_incident_path(), &bytes).unwrap();
        assert!(migrate_legacy_pending(&AtomicBool::new(false)));
        assert_eq!(std::fs::read(&destination).unwrap(), updated_bytes);
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        retire_acknowledged_copy(&destination, &incident.id, &AtomicBool::new(false)).unwrap();
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read(&archive).unwrap(),
            bytes,
            "ACK does not delete the private migration original"
        );
    }

    #[test]
    fn bounded_keyed_discovery_advances_past_unknown_and_mismatched_records() {
        const CHILD: &str = "KONTRA_KEYED_DISCOVERY_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::bounded_keyed_discovery_advances_past_unknown_and_mismatched_records", "--test-threads=1"])
                .env(CHILD, "1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1").status().unwrap();
            assert!(status.success());
            return;
        }
        for sequence in 1..=40 {
            let mut incident = test_incident("0.3.115", "authored-unknown-batch");
            incident.id = format!("{sequence:016x}");
            incident.pid = u32::MAX - sequence;
            incident.host_process = "authored-unknown-host".into();
            assert!(save_pending_incident(&incident));
        }
        let mut proven = test_incident("0.3.115", "authored-late-batch-proof");
        proven.id = "0000000000000041".into();
        proven.kind = IncidentKind::PlatformCrash;
        assert!(save_pending_incident(&proven));
        let wrong_path = reports_dir().join("pending").join("0000000000000040.json");
        assert!(persist_json(&wrong_path, &proven)); // Valid body in another ID's filename.
        let (confirmed, unknown, _) = find_pending_candidates(&AtomicBool::new(false), None, None);
        assert!(confirmed.is_none());
        assert!(unknown.is_some());
        let (confirmed, _, _) = find_pending_candidates(&AtomicBool::new(false), None, None);
        assert_eq!(confirmed.unwrap().id, proven.id);
        assert!(
            wrong_path.exists(),
            "mismatched ownership is neither submitted nor deleted"
        );
        assert!(
            keyed_pending_incident_path("0000000000000001")
                .unwrap()
                .exists()
        );
        assert!(
            find_pending_candidates(&AtomicBool::new(true), None, None)
                .0
                .is_none()
        );
    }

    #[test]
    fn observed_queue_snapshots_cannot_overwrite_newer_same_id_proof() {
        const CHILD: &str = "KONTRA_PENDING_FRESHNESS_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::observed_queue_snapshots_cannot_overwrite_newer_same_id_proof", "--test-threads=1"])
                .env(CHILD, "1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1").status().unwrap();
            assert!(status.success());
            return;
        }
        let mut observed = test_incident("0.3.115", "authored-freshness");
        observed.id = "0123456789abcdef".into();
        observed.host_name = "current host metadata".into();
        observed.pid = u32::MAX - 170;
        assert!(save_pending_incident(&observed));
        let path = keyed_pending_incident_path(&observed.id).unwrap();
        let observed_hash = pending_snapshot(&observed.id).unwrap().1;
        let candidate = RecoveredCandidate {
            marker_path: path.clone(),
            journal_path: None,
            panic_path: panic_marker_path(observed.pid),
            incident: observed.clone(),
        };
        let mut confirmed = observed.clone();
        confirmed.kind = IncidentKind::PlatformCrash;
        confirmed.platform_evidence = "authored first confirmed exception and frame".into();
        let confirmed_bytes = serde_json::to_vec(&confirmed).unwrap();
        let publisher_path = path.clone();
        let published = confirmed_bytes.clone();
        std::thread::spawn(move || {
            buffr_durable_file::publish_private_streaming(
                &publisher_path,
                std::time::Duration::from_secs(2),
                |file| std::io::Write::write_all(file, &published),
            )
        })
        .join()
        .unwrap()
        .unwrap();
        let adopted = candidate.consume(&AtomicBool::new(false)).unwrap();
        assert!(adopted.auto_reportable());
        assert_eq!(adopted.platform_evidence, confirmed.platform_evidence);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            confirmed_bytes,
            "adoption must not republish stale bytes"
        );
        assert!(!save_pending_if_unchanged(&observed, &observed_hash));
        assert_eq!(std::fs::read(&path).unwrap(), confirmed_bytes);
        let prior_hash = pending_snapshot(&confirmed.id).unwrap().1;
        let mut latest = confirmed.clone();
        latest.platform_evidence =
            "authored later complete exception and additional native frame".into();
        let latest_bytes = serde_json::to_vec(&latest).unwrap();
        let publisher_path = path.clone();
        let published = latest_bytes.clone();
        std::thread::spawn(move || {
            buffr_durable_file::publish_private_streaming(
                &publisher_path,
                std::time::Duration::from_secs(2),
                |file| std::io::Write::write_all(file, &published),
            )
        })
        .join()
        .unwrap()
        .unwrap();
        assert!(!save_pending_if_unchanged(&confirmed, &prior_hash));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            latest_bytes,
            "stale confirmed proof cannot overwrite fresher frames"
        );
        assert!(save_pending_incident(&observed)); // Initial re-recovery adopts rather than replaces.
        assert_eq!(std::fs::read(&path).unwrap(), latest_bytes);

        // Confirmed legacy proof safely upgrades an unknown keyed record while
        // preserving its current metadata and both complete private originals.
        let mut keyed_unknown = observed.clone();
        keyed_unknown.id = "fedcba9876543210".into();
        assert!(save_pending_incident(&keyed_unknown));
        let unknown_path = keyed_pending_incident_path(&keyed_unknown.id).unwrap();
        let unknown_bytes = std::fs::read(&unknown_path).unwrap();
        let mut legacy = keyed_unknown.clone();
        legacy.kind = IncidentKind::PlatformCrash;
        legacy.host_name = "older legacy host label".into();
        legacy.platform_evidence = "authored confirmed legacy exception and stack".into();
        let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        buffr_durable_file::publish_private(&pending_incident_path(), &legacy_bytes).unwrap();
        assert!(migrate_legacy_pending(&AtomicBool::new(false)));
        let merged = load_pending_by_id(&legacy.id).unwrap();
        assert!(merged.auto_reportable());
        assert_eq!(merged.host_name, keyed_unknown.host_name);
        assert_eq!(merged.platform_evidence, legacy.platform_evidence);
        assert!(!pending_incident_path().exists());
        for original in [&unknown_bytes, &legacy_bytes] {
            let hash = blake3::hash(original).to_hex().to_string();
            assert_eq!(
                std::fs::read(reports_dir().join("originals").join(format!("{hash}.raw"))).unwrap(),
                *original
            );
            assert!(
                reports_dir()
                    .join("originals")
                    .join(format!("{hash}.json"))
                    .exists()
            );
        }
        // A busy unrelated legacy publisher cannot inhibit independent keyed
        // recovery. The legacy bytes themselves stay untouched.
        buffr_durable_file::publish_private(&pending_incident_path(), &legacy_bytes).unwrap();
        let legacy_lock = buffr_durable_file::acquire_publisher_lock(
            &pending_incident_path(),
            std::time::Duration::ZERO,
        )
        .unwrap();
        let pending = Mutex::new(None);
        assert!(recover_pending_slot(&pending, &AtomicBool::new(false)));
        assert!(
            pending
                .lock_unpoisoned()
                .as_ref()
                .unwrap()
                .auto_reportable()
        );
        assert_eq!(
            std::fs::read(pending_incident_path()).unwrap(),
            legacy_bytes
        );
        drop(legacy_lock);
    }

    #[test]
    fn acknowledged_workers_drain_confirmed_queue_after_permit_drop_without_retry_loops() {
        const CHILD: &str = "KONTRA_COMPLETION_DRAIN_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "support::crash::tests::acknowledged_workers_drain_confirmed_queue_after_permit_drop_without_retry_loops", "--test-threads=1"])
                .env(CHILD, "1").env("KONTRA_REPORT_DIR", directory.path())
                .env("KONTRA_DISABLE_NETWORK", "1").status().unwrap();
            assert!(status.success());
            return;
        }
        let timeout = std::time::Duration::from_secs(30);
        for sequence in 1..=40 {
            let mut unknown = test_incident("0.3.115", "authored-retained-unknown");
            unknown.id = format!("{sequence:016x}");
            unknown.pid = u32::MAX - sequence;
            unknown.host_process = "authored-unknown-host".into();
            assert!(save_pending_incident(&unknown));
        }
        let mut first = test_incident("0.3.115", "authored-first-confirmed");
        first.id = "0000000000000030".into();
        first.kind = IncidentKind::PlatformCrash;
        let mut second = first.clone();
        second.id = "0000000000000031".into();
        assert!(save_pending_incident(&first));
        assert!(save_pending_incident(&second));
        let first_path = keyed_pending_incident_path(&first.id).unwrap();
        let second_path = keyed_pending_incident_path(&second.id).unwrap();
        let second_bytes = std::fs::read(&second_path).unwrap();
        assert!(persist_json(
            &reports_dir().join("pending-cursor.json"),
            &"0000000000000031.json"
        ));
        let (events, event_receiver) = mpsc::channel();
        *TEST_AUTOMATIC_REPORT_OBSERVER.lock_unpoisoned() = Some(events);
        *REPORTER.pending.lock_unpoisoned() = Some(first.clone());
        let (sender, receiver) = mpsc::channel();
        let pending = REPORTER.pending.clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        let worker = std::thread::spawn(move || {
            reporter_worker(
                receiver,
                Arc::new(Mutex::new(None)),
                pending,
                Arc::new(AtomicBool::new(false)),
                None,
                sessions_dir().join("unused.json"),
                bootstrap_journal_path("unused"),
                "unused".into(),
                worker_stopping,
            )
        });
        *REPORTER.runtime.lock_unpoisoned() = Some(ReporterRuntime {
            control_sender: sender,
            worker,
            stopping,
        });
        let successful = || {
            Ok((200, r#"{"ok":true,"report_id":"authored","diagnostics_sha256":"authored-full-evidence"}"#.into()))
        };
        let (entered, began) = mpsc::channel();
        let (outcome, outcome_receiver) = mpsc::channel();
        let (release, released) = mpsc::channel();
        super::super::report::test_detached_automatic_delivery(
            first.id.clone(),
            successful(),
            entered,
            outcome,
            released,
        )
        .unwrap();
        began.recv_timeout(timeout).unwrap();
        assert!(outcome_receiver.recv_timeout(timeout).unwrap());
        assert!(super::super::report::test_automatic_report_is_busy());
        assert!(
            matches!(
                event_receiver.recv_timeout(std::time::Duration::from_millis(25)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "acknowledgement cannot advance while its worker still owns the permit"
        );
        release.send(()).unwrap();
        let mut scans = 0;
        loop {
            match event_receiver.recv_timeout(timeout).unwrap() {
                TestAutomaticEvent::Scanned(_, _) => scans += 1,
                TestAutomaticEvent::Ready(id, busy) => {
                    assert_eq!(id, second.id);
                    assert!(!busy, "continuation runs only after permit release");
                    break;
                }
            }
        }
        assert!(
            scans <= 4,
            "one wrap and bounded batches reach proof past 40 unknown records"
        );
        assert!(!first_path.exists());
        assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        assert_eq!(
            REPORTER.pending.lock_unpoisoned().as_ref().unwrap().id,
            second.id
        );
        // Wrong digest and offline completion never acknowledge or enqueue a retry.
        for failure in [
            Ok((
                200,
                r#"{"ok":true,"report_id":"authored","diagnostics_sha256":"wrong"}"#.into(),
            )),
            Err("authored offline delivery".into()),
        ] {
            let (entered, began) = mpsc::channel();
            let (outcome, outcome_receiver) = mpsc::channel();
            let (release, released) = mpsc::channel();
            super::super::report::test_detached_automatic_delivery(
                second.id.clone(),
                failure,
                entered,
                outcome,
                released,
            )
            .unwrap();
            began.recv_timeout(timeout).unwrap();
            assert!(!outcome_receiver.recv_timeout(timeout).unwrap());
            release.send(()).unwrap();
            let deadline = std::time::Instant::now() + timeout;
            while super::super::report::test_automatic_report_is_busy() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(matches!(
                event_receiver.recv_timeout(std::time::Duration::from_millis(25)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
            assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        }
        // A valid ACK whose local retirement is busy also stops continuation.
        let lock =
            buffr_durable_file::acquire_publisher_lock(&second_path, std::time::Duration::ZERO)
                .unwrap();
        let (entered, began) = mpsc::channel();
        let (outcome, outcome_receiver) = mpsc::channel();
        let (release, released) = mpsc::channel();
        super::super::report::test_detached_automatic_delivery(
            second.id.clone(),
            successful(),
            entered,
            outcome,
            released,
        )
        .unwrap();
        began.recv_timeout(timeout).unwrap();
        assert!(outcome_receiver.recv_timeout(timeout).unwrap());
        release.send(()).unwrap();
        assert!(matches!(
            event_receiver.recv_timeout(std::time::Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert_eq!(std::fs::read(&second_path).unwrap(), second_bytes);
        drop(lock);
        // An explicitly initiated later attempt succeeds; the finite scan stops
        // after one wrap with only unknown originals left, without uploading them.
        let (entered, began) = mpsc::channel();
        let (outcome, outcome_receiver) = mpsc::channel();
        let (release, released) = mpsc::channel();
        super::super::report::test_detached_automatic_delivery(
            second.id.clone(),
            successful(),
            entered,
            outcome,
            released,
        )
        .unwrap();
        began.recv_timeout(timeout).unwrap();
        assert!(outcome_receiver.recv_timeout(timeout).unwrap());
        release.send(()).unwrap();
        let mut scans = 0;
        loop {
            match event_receiver.recv_timeout(timeout).unwrap() {
                TestAutomaticEvent::Scanned(wrapped, exhausted) => {
                    scans += 1;
                    if wrapped && exhausted {
                        break;
                    }
                }
                TestAutomaticEvent::Ready(id, _) => {
                    panic!("unexpected immediate report retry for {id}")
                }
            }
        }
        assert!(scans <= 4);
        assert!(!second_path.exists());
        assert!(
            keyed_pending_incident_path("0000000000000001")
                .unwrap()
                .exists()
        );
        assert!(matches!(
            event_receiver.recv_timeout(std::time::Duration::from_millis(25)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        REPORTER.shutdown();
        *TEST_AUTOMATIC_REPORT_OBSERVER.lock_unpoisoned() = None;
    }

    #[test]
    fn original_archive_failure_and_cancellation_preserve_source_and_prior_archive() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("session.dfr");
        let archive = directory.path().join("original.dfr");
        std::fs::write(&source, b"authored original").unwrap();
        std::fs::write(&archive, b"prior private archive").unwrap();
        let expected = LocalJournalEvidence {
            bytes: 16,
            blake3: blake3::hash(b"different source").to_hex().to_string(),
            ..Default::default()
        };
        assert!(
            archive_original(&source, &archive, Some(&expected), &AtomicBool::new(false)).is_err()
        );
        assert!(archive_original(&source, &archive, None, &AtomicBool::new(true)).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"authored original");
        assert_eq!(std::fs::read(&archive).unwrap(), b"prior private archive");
    }

    #[test]
    fn delayed_evidence_only_upgrades_unclean_exit() {
        let late_dump = CrashEvidence {
            disposition: EvidenceDisposition::Crash,
            signature: "late-dump".to_string(),
            text: "correlated crash dump".to_string(),
        };
        let mut unclean = incident(IncidentKind::UncleanExit, vec![]);
        let original = unclean.preview_diagnostics();
        assert!(apply_delayed_crash_evidence(&mut unclean, &late_dump));
        let expected = (
            IncidentKind::PlatformCrash,
            "late-dump",
            "correlated crash dump",
        );
        assert_eq!(evidence_state(&unclean), expected);
        let refreshed = unclean.preview_diagnostics();
        let base = format!("{original}\n\nCurrent KONTRA diagnostics:\n");
        assert_eq!(
            current_diagnostics_body(&format!("{base}renderer=wgpu"), &refreshed),
            "renderer=wgpu"
        );
        assert!(current_diagnostics_body(&base, &refreshed).is_empty());

        let mut no_evidence = incident(IncidentKind::UncleanExit, vec![]);
        assert!(!apply_delayed_crash_evidence(
            &mut no_evidence,
            &CrashEvidence::default()
        ));
        assert_eq!(
            evidence_state(&no_evidence),
            (IncidentKind::UncleanExit, "sig", "")
        );
        for mut confirmed in [
            incident(IncidentKind::PlatformCrash, vec![]),
            panicked(vec![]),
        ] {
            let kind = confirmed.kind;
            assert!(!apply_delayed_crash_evidence(&mut confirmed, &late_dump));
            assert_eq!(evidence_state(&confirmed), (kind, "sig", ""));
        }
    }

    /// Image UUIDs and slides survive the panic marker round trip (legacy markers still decode)
    /// and reach the preview.
    #[test]
    fn panic_marker_images_survive_serialization_into_the_preview() {
        let image = LoadedImage {
            path: "/Library/Audio/Plug-Ins/CLAP/KONTRA.clap/Contents/MacOS/KONTRA".to_string(),
            load_address: 0x1_0a2c_0000,
            slide: 0x2c0_0000,
            uuid: "69A612E8-EAE9-3A96-AA21-EA4729DA52B0".to_string(),
        };
        let incident = panicked(vec![image.clone()]);
        let encoded = serde_json::to_string(incident.panic.as_ref().unwrap()).unwrap();
        let decoded: PanicMarker = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.images, [image]);
        let legacy: PanicMarker = serde_json::from_str(
            r#"{"at":1,"thread":"gui","message":"boom","location":"src/editor.rs:1"}"#,
        )
        .unwrap();
        assert!(legacy.images.is_empty());
        let rendered = incident.preview_diagnostics();
        assert!(
            rendered.contains("0x10a2c0000 46137344 69A612E8-EAE9-3A96-AA21-EA4729DA52B0 /Library/Audio/Plug-Ins/CLAP/KONTRA.clap/Contents/MacOS/KONTRA"),
            "{rendered}"
        );
    }

    /// The last panic-hook owner may drop while unwinding without deadlocking.

    #[test]
    fn pinning_the_own_module_succeeds_and_repeats() {
        assert_eq!(pin_own_module(), Ok(()));
        assert_eq!(pin_own_module(), Ok(()));
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        for _ in 0..2 {
            assert_eq!(
                pin_module_containing(libc::getpid as *const std::ffi::c_void),
                Ok(())
            );
        }
    }

    /// Unload returns while work that waits on the user (an open file dialog) is still blocked.
    #[test]
    fn unload_does_not_wait_for_a_blocked_detached_worker() {
        let session = crate::support::register_crash_session();
        let (release, blocked) = mpsc::channel::<()>();
        spawn_detached("dialog-like-test", move || {
            let _ = blocked.recv();
        })
        .unwrap();
        let (unloaded, done) = mpsc::channel();
        std::thread::spawn(move || {
            drop(session);
            let _ = unloaded.send(());
        });
        assert!(
            done.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "unload waited on a worker blocked on the user"
        );
        drop(release);
    }

    /// Finished support work is joined when more is started, not kept until unload.
    #[test]
    fn starting_support_work_joins_what_has_finished() {
        let mut workers = SupportWorkers::default();
        workers.spawn("finished-test", || {}).unwrap();
        while !workers
            .handles
            .iter()
            .all(std::thread::JoinHandle::is_finished)
        {
            std::thread::yield_now();
        }
        let (release, wait) = mpsc::channel::<()>();
        workers
            .spawn("running-test", move || {
                let _ = wait.recv();
            })
            .unwrap();
        assert_eq!(workers.handles.len(), 1);
        drop(release);
        join_support_workers(workers.close_and_take());
    }

    /// Closing joins running support work and rejects work spawned after the last instance.
    #[test]
    fn closed_support_workers_join_running_work_and_reject_late_work() {
        let workers = Mutex::new(SupportWorkers::default());
        let completed = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&completed);
        workers
            .lock_unpoisoned()
            .spawn("running-test", move || done.store(true, Ordering::Release))
            .unwrap();
        let handles = workers.lock_unpoisoned().close_and_take();
        join_support_workers(handles);
        assert!(completed.load(Ordering::Acquire));

        let ran = Arc::new(AtomicBool::new(false));
        let late = Arc::clone(&ran);
        let spawned = workers
            .lock_unpoisoned()
            .spawn("late-auto-report-test", move || {
                late.store(true, Ordering::Release)
            });
        assert!(spawned.is_err(), "late work must be rejected");
        assert!(!ran.load(Ordering::Acquire));
        let workers = workers.lock_unpoisoned();
        assert!(workers.closed && workers.handles.is_empty());
    }

    /// Shutdown must wait for a worker that stalls after acknowledging a registration.
    #[test]
    fn shutdown_waits_for_reporter_stalled_after_registration_acknowledgement() {
        use std::time::Duration;
        let reporter = CrashReporter {
            runtime: Mutex::new(None),
            recorder: Mutex::new(None),
            pending: Arc::new(Mutex::new(None)),
            support_workers: Mutex::new(SupportWorkers::default()),
            ready: Arc::new(AtomicBool::new(false)),
            marker: Arc::new(Mutex::new(None)),
            session_path: Mutex::new(None),
        };
        let (control_sender, control_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let Ok(ReporterControl::Register(_, acknowledge)) = control_receiver.recv() else {
                panic!("expected registration");
            };
            acknowledge.send(true).unwrap();
            release_receiver.recv().unwrap();
            let Ok(ReporterControl::Shutdown) = control_receiver.recv() else {
                panic!("expected shutdown");
            };
        });
        let stopping = Arc::new(AtomicBool::new(false));
        *reporter.runtime.lock_unpoisoned() = Some(ReporterRuntime {
            control_sender,
            worker,
            stopping: Arc::clone(&stopping),
        });
        let (register_acknowledge, registered) = mpsc::sync_channel(1);
        assert!(reporter.send(ReporterControl::Register(1, register_acknowledge)));
        assert!(registered.recv().unwrap());

        let (finished_sender, finished_receiver) = mpsc::channel();
        let shutdown = std::thread::spawn(move || {
            reporter.shutdown();
            finished_sender.send(()).unwrap();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !stopping.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(stopping.load(Ordering::Acquire), "shutdown never started");
        let finished_early = finished_receiver
            .recv_timeout(Duration::from_secs(3))
            .is_ok();
        release_sender.send(()).unwrap();
        shutdown.join().unwrap();
        assert!(
            !finished_early,
            "shutdown returned while the worker was live"
        );
    }

    #[test]
    fn unsaved_panic_marker_falls_back_to_stderr() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("not-a-directory");
        std::fs::write(&blocker, b"file").unwrap();
        let marker = PanicMarker {
            at: 1,
            thread: "worker".to_owned(),
            message: "evidence that must survive".to_owned(),
            location: "src/lib.rs:1:1".to_owned(),
            images: Vec::new(),
        };
        let mut fallback = Vec::new();
        persist_panic_marker(&blocker.join("panic.json"), &marker, &mut fallback);
        let fallback = String::from_utf8(fallback).unwrap();
        assert!(
            fallback.contains("evidence that must survive"),
            "{fallback}"
        );
        assert!(fallback.contains("src/lib.rs:1:1"), "{fallback}");
    }

    /// Only a real crash is auto-reported and fingerprinted in the crash namespace: an unclean
    /// exit, a completed deactivate, or a finished unload is not; a panic always is.
    #[test]
    fn only_real_crashes_are_auto_reported_in_the_crash_namespace() {
        use IncidentKind::{PlatformCrash, UncleanExit};
        let mut deactivating = vec![
            host_record("plugin_initialize", Phase::Started),
            host_record("plugin_initialize", Phase::Completed),
            host_record("plugin_deactivate", Phase::Started),
        ];
        let events = VecDeque::from(deactivating.clone());
        assert!(!journal_ended_cleanly(&events) && !host_unloaded_cleanly(&events));
        deactivating.push(host_record("plugin_deactivate", Phase::Completed));
        assert!(host_unloaded_cleanly(&VecDeque::from(deactivating.clone())));
        let mut finished_unload = incident(PlatformCrash, vec![]);
        finished_unload.host_finished_unload = true;
        let mut panic_after_deactivate = panicked(vec![]);
        panic_after_deactivate.events = [host_record("plugin_deactivate", Phase::Completed)].into();

        let unclean_ns = Some("kontra-unclean-exit-v1");
        for (name, incident, reportable, namespace) in [
            ("unclean", incident(UncleanExit, vec![]), false, unclean_ns),
            (
                "crash",
                incident(PlatformCrash, vec![]),
                true,
                Some("kontra-crash-v2"),
            ),
            (
                "deactivated",
                incident(PlatformCrash, deactivating),
                false,
                unclean_ns,
            ),
            ("finished unload", finished_unload, false, None),
            ("panic after deactivate", panic_after_deactivate, true, None),
        ] {
            assert_eq!(incident.auto_reportable(), reportable, "{name}");
            if let Some(namespace) = namespace {
                assert_eq!(incident.fingerprint_namespace(), namespace, "{name}");
            }
        }
        assert_ne!(
            incident(UncleanExit, vec![]).crash_fingerprint().unwrap(),
            incident(PlatformCrash, vec![]).crash_fingerprint().unwrap(),
            "identical material must not collide across the two namespaces"
        );
    }

    /// A synchronously written teardown must read back as clean, with the host identity kept.
    #[test]
    fn a_synchronously_written_teardown_reads_back_as_clean() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let mut marker = new_session_marker(1, "test-run-token");
        marker.apply_lifecycle(2, InstanceLifecycle::Registered, None, None);
        let (host, api) = (Some("REAPER".to_string()), Some("CLAP".to_string()));
        marker.apply_lifecycle(1, InstanceLifecycle::Initializing, host, api);
        marker.apply_lifecycle(1, InstanceLifecycle::Active, None, None);
        assert!(persist_json(&path, &marker));
        let recovered: SessionMarker = read_json(&path).unwrap();
        assert!(
            !session_ended_cleanly(&recovered),
            "a live instance is unclean"
        );
        assert_eq!(
            (recovered.host_name.as_str(), recovered.plugin_api.as_str()),
            ("REAPER", "CLAP")
        );

        marker.apply_lifecycle(1, InstanceLifecycle::Inactive, None, None);
        marker.apply_lifecycle(2, InstanceLifecycle::Inactive, None, None);
        assert!(persist_json(&path, &marker));
        let recovered: SessionMarker = read_json(&path).unwrap();
        assert!(
            session_ended_cleanly(&recovered),
            "a clean teardown is not a crash"
        );
        assert!(marker_finished_unload(&recovered));
    }

    /// Report #218: a marker recovered inside another DAW must not carry that DAW's diagnostics.
    #[test]
    fn diagnostics_from_a_different_host_are_withheld() {
        assert_eq!(comparable_process_name("REAPER.exe"), "reaper");
        assert_eq!(comparable_process_name("/usr/bin/reaper"), "reaper");
        let mut incident = incident(IncidentKind::UncleanExit, vec![]);
        let attached = attributable_current_diagnostics(&incident, "renderer=wgpu host=FL64.exe");
        assert!(
            !attached.contains("renderer=wgpu") && attached.contains("reaper.exe"),
            "{attached}"
        );
        incident.host_process = current_process_name();
        assert_eq!(
            attributable_current_diagnostics(&incident, "renderer=wgpu"),
            "renderer=wgpu"
        );
    }

    /// Eliding the middle of a long body used to cut the platform evidence out.
    #[test]
    fn platform_evidence_is_never_elided_out_of_the_payload() {
        let evidence = format!("Crashed Thread: 4\n{}", "frame ".repeat(1_000));
        let preview = "p".repeat(MAX_EVIDENCE_CHARS * 2);
        let assembled = assemble_enriched_diagnostics(&preview, &evidence, "renderer=wgpu");
        assert!(assembled.chars().count() <= MAX_EVIDENCE_CHARS);
        assert!(assembled.contains(&evidence) && assembled.contains("renderer=wgpu"));
        assert!(assembled.contains("Correlated operating-system or host evidence:"));
    }

    /// Report #235: a failed incident save must not ship a crash payload without its evidence.
    #[test]
    fn a_report_uses_the_in_memory_incident_when_the_file_is_gone() {
        let mut incident = incident(IncidentKind::PlatformCrash, vec![]);
        incident.id = format!("in-memory-{}", now_unix());
        incident.platform_evidence = "Crashed Thread: 4 · buffr::spectrogram".to_string();
        incident.host_process = current_process_name();
        let id = incident.id.clone();
        let restore = (*REPORTER.pending.lock_unpoisoned()).replace(incident);
        let enriched = enrich_diagnostics(&id, "renderer=wgpu");
        *REPORTER.pending.lock_unpoisoned() = restore;
        assert!(
            enriched.contains("Crashed Thread: 4 · buffr::spectrogram"),
            "{enriched}"
        );
        assert!(enriched.contains("Classification: platform_crash"));
    }

    #[test]
    fn evidence_tail_drops_noise_and_keeps_the_failed_initialize() {
        let mut records = (0..256)
            .map(|sequence| Record {
                sequence,
                subsystem: Subsystem::Ui,
                action: "pointer".to_string(),
                phase: Phase::Event,
                importance: Importance::Noise,
                ..Record::default()
            })
            .collect::<Vec<_>>();
        records.push(Record {
            sequence: 256,
            ..host_record("plugin_initialize", Phase::Failed)
        });
        let evidence = evidence_tail(records);
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            (evidence[0].action.as_str(), evidence[0].phase),
            ("plugin_initialize", Phase::Failed)
        );
    }

    /// RPT-13/14: the preview names the build that recorded the incident and flags a previous
    /// install; user lines stay apart from diagnostics.
    #[test]
    fn the_preview_names_the_recording_build_and_separates_user_lines() {
        let mut stale = test_incident("1.0.72", "deadbeefcafe");
        assert_ne!(stale.version, env!("CARGO_PKG_VERSION"));
        stale.events = [Record {
            sequence: 1,
            subsystem: Subsystem::Ui,
            action: "capture_state".to_string(),
            phase: Phase::Started,
            user_line: "Recording started.".to_string(),
            detail: "recording=true retained_s=0.00".to_string(),
            code: "capture.state".to_string(),
            file: "src/editor/panels/log.rs".to_string(),
            line: 10,
            importance: Importance::Evidence,
            ..Record::default()
        }]
        .into();
        let preview = stale.preview_diagnostics();
        for present in [
            "Version that recorded this incident: 1.0.72 (recorded by a previous install)",
            "Build ID that recorded this incident: deadbeefcafe",
            "What KONTRA showed:\nRecording started.",
            "loc=src/editor/panels/log.rs:10",
            "code=capture.state",
        ] {
            assert!(preview.contains(present), "{present}: {preview}");
        }
        for absent in ["\nVersion: ", "\nBuild ID: ", "What KONTRA showed:\n#00001"] {
            assert!(!preview.contains(absent), "{absent}: {preview}");
        }
        let current =
            test_incident("1.0.72", crate::build_info::BUILD.build_hash).preview_diagnostics();
        assert!(current.contains("Version that recorded this incident: 1.0.72\n"));
    }
}
