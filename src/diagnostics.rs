//! Loading diagnostics. Only loaders and inspection commands write here;
//! the audio callback must never serialize, lock the journal, or touch disk.
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const LOG_LIMIT: u64 = 8 * 1024 * 1024;
static NEXT_LOAD: AtomicU64 = AtomicU64::new(1);
static JOURNAL: OnceLock<Mutex<Journal>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct Journal {
    path: PathBuf,
    file: Option<File>,
    error: Option<String>,
}
impl Journal {
    fn open() -> Self {
        #[cfg(not(test))]
        let root = std::env::var_os("KONTRA_LOG_DIR")
            .map(PathBuf::from)
            .or_else(|| crate::cache::dir().map(|p| p.join("logs")))
            .unwrap_or_else(|| std::env::temp_dir().join("kontra-logs"));
        #[cfg(test)]
        let root = std::env::temp_dir().join(format!("kontra-test-{}/logs", std::process::id()));
        let path = root.join(format!("session-{}-{}.jsonl", now(), std::process::id()));
        let _ = LOG_PATH.set(path.clone());
        let result = std::fs::create_dir_all(&root)
            .and_then(|_| OpenOptions::new().create(true).append(true).open(&path));
        match result {
            Ok(file) => {
                eprintln!("KONTRA diagnostics log: {}", path.display());
                Self {
                    path,
                    file: Some(file),
                    error: None,
                }
            }
            Err(e) => {
                eprintln!("KONTRA diagnostics: {}: {e}", path.display());
                Self {
                    path,
                    file: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }
    fn write(&mut self, event: &Value) -> std::io::Result<()> {
        if self
            .file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() >= LOG_LIMIT)
        {
            self.file.take();
            let previous = self.path.with_extension("previous.jsonl");
            if previous.exists() {
                std::fs::remove_file(&previous)?;
            }
            std::fs::rename(&self.path, previous)?;
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        let file = self.file.as_mut().ok_or_else(|| {
            std::io::Error::other(
                self.error
                    .clone()
                    .unwrap_or_else(|| "Log is unavailable".into()),
            )
        })?;
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        file.write_all(&bytes)
    }
}

fn emit(mut event: Value) -> Option<String> {
    event["timestamp_ms"] = json!(now());
    let mut log = JOURNAL
        .get_or_init(|| Mutex::new(Journal::open()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(e) = log.write(&event) {
        let error = e.to_string();
        if log.error.as_deref() != Some(&error) {
            eprintln!("KONTRA diagnostics: {}: {error}", log.path.display());
        }
        log.error = Some(error.clone());
        return Some(error);
    }
    None
}

/// Called by the loader after a diagnostic snapshot returns from the audio thread.
pub(crate) fn runtime(
    path: &Path,
    program: u32,
    part: usize,
    epoch: u64,
    load_id: Option<&str>,
    issues: &[Value],
) {
    for issue in issues {
        emit(
            json!({"event":"runtime_issue","load_id":load_id,"path":path,"program":program,"part":part,"script_epoch":epoch,"data":issue}),
        );
    }
}

pub fn log_path() -> Option<PathBuf> {
    // The editor must not wait behind a worker writing a slow log device.
    LOG_PATH.get().cloned()
}

/// A resource/catalog operation on a loader, never during playback.
pub(crate) fn resource(instrument: &Path, name: &str, error: &str) {
    emit(json!({"event":"resource_issue","path":instrument,"resource":name,"message":error}));
}

/// One instrument attempt, including stages written *before* blocking work.
/// A killed or stuck loader therefore leaves the responsible stage on disk.
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
        this.record("load_started", json!({}));
        if let Some(path) = log_path() {
            this.report["log_path"] = json!(path);
        }
        this
    }
    fn record(&mut self, event: &str, data: Value) {
        if let Some(error) = emit(
            json!({"event":event,"load_id":self.report["load_id"],"path":self.report["path"],"program":self.report["program"],"part":self.report["part"],"stage":self.stage,"data":data}),
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
        let message = message.into();
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
            self.record(
                "load_incomplete",
                json!({"elapsed_ms":self.started.elapsed().as_secs_f64()*1000.}),
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
    fn traces_keep_failures_and_the_last_stage_without_audio_thread_work() {
        let mut t = LoadTrace::new(Path::new("Missing Harp.nki"), 2, Some(3));
        t.stage("import");
        t.detail("zones", 17);
        t.issue("artwork", "missing", "knob.png not found");
        let report = t.finish("loaded");
        assert_eq!(report["status"], "partial");
        assert_eq!(report["details"]["zones"], 17);
        let path = log_path().unwrap();
        let lines = std::fs::read_to_string(path).unwrap();
        let rows: Vec<Value> = lines
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let rows: Vec<_> = rows
            .iter()
            .filter(|r| r["load_id"] == report["load_id"])
            .collect();
        assert!(
            rows.iter()
                .any(|r| r["event"] == "stage_started" && r["stage"] == "import")
        );
        assert!(
            rows.iter()
                .any(|r| r["event"] == "issue" && r["data"]["message"] == "knob.png not found")
        );
        assert_eq!(rows.last().unwrap()["data"]["status"], "partial");
        let mut unfinished = LoadTrace::new(Path::new("unfinished.nki"), 0, None);
        unfinished.stage("samples");
        let id = unfinished.report["load_id"].clone();
        drop(unfinished);
        let rows = std::fs::read_to_string(log_path().unwrap()).unwrap();
        assert!(
            rows.lines()
                .map(|s| serde_json::from_str::<Value>(s).unwrap())
                .any(|r| r["load_id"] == id
                    && r["event"] == "load_incomplete"
                    && r["stage"] == "samples")
        );

        // Exercise the actual rotation branch without generating 8 MiB of events.
        let path =
            std::env::temp_dir().join(format!("kontra-rotation-{}.jsonl", std::process::id()));
        let file = File::create(&path).unwrap();
        file.set_len(LOG_LIMIT).unwrap();
        let mut journal = Journal {
            path: path.clone(),
            file: Some(file),
            error: None,
        };
        journal.write(&json!({"event":"after_rotation"})).unwrap();
        let row: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(row["event"], "after_rotation");
        drop(journal);
        std::fs::remove_file(path.with_extension("previous.jsonl")).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
