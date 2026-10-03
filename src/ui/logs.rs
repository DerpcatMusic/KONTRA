//! Global loader/runtime journal. Snapshot copies and report export stay off the UI thread.
use super::{Cx, picker, theme::*};
use crate::diagnostics::{self, DiagnosticSnapshot, ExportStatus, LogEvent, LogLevel};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

// One SMALL caption and one TEXT reason, including both 2 px insets.
const ROW: f64 = 36.;
static WAKE: AtomicU64 = AtomicU64::new(0);

/// Completed background UI operations also wake an idle plugin editor.
pub(super) fn wake_worker() { WAKE.fetch_add(1, Ordering::Release); }

pub fn wake() -> u64 {
    WAKE.load(Ordering::Acquire)
}

#[derive(Default)]
struct Reader {
    busy: AtomicBool,
    ready: AtomicBool,
    answer: Mutex<Option<Result<DiagnosticSnapshot, String>>>,
}

pub struct State {
    reader: Arc<Reader>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    export_thread: Option<std::thread::JoinHandle<()>>,
    export_answer: Arc<Mutex<Option<Result<u64, String>>>>,
    copy_thread: Option<std::thread::JoinHandle<()>>,
    copy_answer: Arc<Mutex<Option<Result<String, String>>>>,
    copy_ready: Option<String>,
    copy_message: Option<String>,
    copy_error: Option<String>,
    snapshot: Option<Arc<DiagnosticSnapshot>>,
    requested: Option<u64>,
    read_error: Option<String>,
    search: String,
    levels: [bool; 4],
    filtered: Option<(u64, String, [bool; 4])>,
    matches: Vec<usize>,
    selected: Option<u64>,
    detail: Option<(u64, Arc<str>)>,
    pub(super) about: bool,
    y: f64,
    reveal: bool,
    anchor: Option<(u64, f64)>,
    preview: bool,
    destination: String,
    redact: bool,
    export: Option<u64>,
    export_error: Option<String>,
    folder_error: Option<String>,
    folder_picker: Arc<picker::Picker>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            reader: Arc::default(),
            reader_thread: None,
            export_thread: None,
            export_answer: Arc::default(),
            copy_thread: None,
            copy_answer: Arc::default(),
            copy_ready: None,
            copy_message: None,
            copy_error: None,
            snapshot: None,
            requested: None,
            read_error: None,
            search: String::new(),
            levels: [false, false, true, true],
            filtered: None,
            matches: Vec::new(),
            selected: None,
            detail: None,
            about: false,
            y: 0.,
            reveal: false,
            anchor: None,
            preview: false,
            destination: String::new(),
            redact: true,
            export: None,
            export_error: None,
            folder_error: None,
            folder_picker: Arc::default(),
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        // The plugin library cannot unload while one of its workers is running.
        for handle in [self.reader_thread.take(), self.export_thread.take(), self.copy_thread.take()]
            .into_iter()
            .flatten()
        {
            let _ = handle.join();
        }
    }
}
impl State {
    pub(super) fn for_load(&mut self, load: &str) {
        self.search = if load.is_empty() { String::new() } else { format!("load:{load}") };
        self.levels = [true; 4];
    }

    fn refresh(&mut self, params: &Arc<SamplerParams>) {
        if let Some(picker::Picked::Revealed(result)) = self.folder_picker.take() {
            self.folder_error = result.err();
        }
        if self.copy_thread.is_some() {
            if let Some(answer) = super::lock(&self.copy_answer).take() {
                let _ = self.copy_thread.take().unwrap().join();
                match answer {
                    Ok(text) => self.copy_ready = Some(text),
                    Err(error) => {
                        self.copy_message = None;
                        self.copy_error = Some(error);
                    }
                }
            }
        }
        if self.export_thread.is_some() {
            if let Some(answer) = super::lock(&self.export_answer).take() {
                let _ = self.export_thread.take().unwrap().join();
                match answer {
                    Ok(id) => {
                        self.export = Some(id);
                        self.export_error = None;
                    }
                    Err(error) => self.export_error = Some(error),
                }
            }
        }
        if self.reader.ready.swap(false, Ordering::AcqRel) {
            if let Some(handle) = self.reader_thread.take() {
                let _ = handle.join();
            }
            if let Some(answer) = super::lock(&self.reader.answer).take() {
                match answer {
                    Ok(snapshot) => {
                        self.snapshot = Some(Arc::new(snapshot));
                        self.read_error = None;
                    }
                    Err(error) => self.read_error = Some(error),
                }
            }
        }
        let revision = diagnostics::revision();
        if self.requested == Some(revision) || self.reader.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        self.requested = Some(revision);
        let reader = self.reader.clone();
        let keepalive = params.clone();
        let spawned = std::thread::Builder::new()
            .name("kontra-log-view".into())
            .spawn(move || {
                // Keep Shared's journal lease alive until this worker has finished.
                let _keepalive = keepalive;
                let answer = std::panic::catch_unwind(diagnostics::snapshot).map_err(|_| {
                    "Could not read the diagnostic journal. Retry refresh.".to_owned()
                });
                *super::lock(&reader.answer) = Some(answer);
                reader.ready.store(true, Ordering::Release);
                reader.busy.store(false, Ordering::Release);
                WAKE.fetch_add(1, Ordering::Release);
            });
        match spawned {
            Ok(handle) => self.reader_thread = Some(handle),
            Err(error) => {
                self.reader.busy.store(false, Ordering::Release);
                self.read_error = Some(format!("Could not start log reader: {error}"));
            }
        }
    }

    fn filter(&mut self, snapshot: &DiagnosticSnapshot) -> bool {
        let key = (
            snapshot.revision,
            self.search.clone(),
            self.levels,
        );
        if self.filtered.as_ref() == Some(&key) {
            return false;
        }
        let needle = self.search.to_lowercase();
        let words: Vec<_> = needle.split_whitespace().collect();
        let criteria_changed = self.filtered.as_ref().is_none_or(|old| {
            (&old.1, old.2) != (&key.1, key.2)
        });
        self.matches = snapshot
            .events
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(n, event)| {
                if !self.levels[level_index(event.level)] {
                    return None;
                }
                if !words.is_empty() {
                    let hay = format!(
                        "{} {} {} {} {} {} {} {} {} {} {:?} {:?} {}",
                        event.module,
                        event.event,
                        event.stage.as_deref().unwrap_or_default(),
                        event.code.as_deref().unwrap_or_default(),
                        event.reason.as_deref().unwrap_or_default(),
                        event.library.as_deref().unwrap_or_default(),
                        event.path.as_deref().unwrap_or_default(),
                        event.load_id.as_deref().unwrap_or_default(),
                        event.details,
                        level_name(event.level), event.script_slot, event.line,
                        event.outcome.as_deref().unwrap_or_default()
                    )
                    .to_lowercase();
                    if !words.iter().all(|w| match w.strip_prefix("load:") {
                        Some(load) => event.load_id.as_deref().is_some_and(|id| id.to_lowercase() == load),
                        None => hay.contains(w),
                    }) {
                        return None;
                    }
                }
                Some(n)
            })
            .collect();
        self.filtered = Some(key);
        if self.selected.is_some_and(|seq| {
            !self
                .matches
                .iter()
                .any(|&n| snapshot.events[n].sequence == seq)
        }) {
            self.selected = None;
        }
        if criteria_changed {
            self.y = 0.;
            self.anchor = None;
        } else if self.y > 0.
            && let Some((seq, offset)) = self.anchor
            && let Some(n) = self
                .matches
                .iter()
                .position(|&n| snapshot.events[n].sequence == seq)
        {
            self.y = n as f64 * ROW + offset;
        }
        true
    }
}

pub fn view(ui: &mut Ui, cx: &mut Cx) -> El {
    cx.state.logs.refresh(cx.p);
    draw(ui, &mut cx.state.logs, cx.p)
}

fn field(ui: &mut Ui, id: &str, text: &mut String, label: &str) -> El {
    col![
        row![caption(label).fill(secondary())].justify(Justify::Start),
        text_input(ui, id, text)
            .el
            .named(label.to_owned())
            .h(CONTROL)
            .w(Len::Pct(100.))
    ]
    .gap(2)
    .align(Align::Stretch)
    .flex(1)
    .min_w(0)
}
fn level_index(level: LogLevel) -> usize {
    match level {
        LogLevel::Debug => 0,
        LogLevel::Info => 1,
        LogLevel::Warning => 2,
        LogLevel::Error => 3,
    }
}
fn level_name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "Debug",
        LogLevel::Info => "Info",
        LogLevel::Warning => "Warning",
        LogLevel::Error => "Error",
    }
}
fn time(ms: u64) -> String {
    let sec = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03} UTC",
        sec / 3600 % 24,
        sec / 60 % 60,
        sec % 60,
        ms % 1000
    )
}
fn filename(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path)
}

fn script_context(data: &serde_json::Value, source_expected: bool) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    if let Some(excerpt) = diagnostics::excerpt_text(data) {
        let _ = writeln!(text, "Script source context (numbered lines; > marks the fault)\n{excerpt}\nColumns refer to the original source; path redaction may alter displayed text.");
    } else if let Some(reason) = data["source_excerpt_unavailable"].as_str() {
        let _ = writeln!(text, "Script source context unavailable: {reason}.");
    } else if source_expected {
        text.push_str("Script source context unavailable: no excerpt was retained for this event.\n");
    }
    let action = &data["last_action"];
    if let Some(callback) = action["callback"].as_str() {
        let _ = write!(text, "Callback: {callback}");
        for (key, label) in [
            ("callback_id", "callback ID"), ("event_id", "event ID"),
            ("note", "note"), ("velocity", "velocity"), ("controller", "controller"),
            ("value", "value"), ("ui_control", "UI control"),
            ("listener_signal", "listener signal"), ("rpn_address", "RPN address"),
            ("async_id", "async ID"), ("async_status", "async status"),
        ] {
            if let Some(value) = action[key].as_i64() {
                let _ = write!(text, " · {label} {value}");
            }
        }
        if let Some(channel) = action["midi_channel"].as_u64() {
            let _ = write!(text, " · MIDI channel {}", channel.saturating_add(1));
        }
        text.push('\n');
    }
    let note = &data["context"]["MidiNote"];
    if let (Some(builtin), Some(argument), Some(value)) =
        (note["builtin"].as_str(), note["argument"].as_u64(), note["value"].as_i64())
    {
        let _ = writeln!(text, "Argument: {builtin} · argument {argument} · value {value}");
    }
    let array = &data["array"];
    if let (Some(name), Some(index), Some(length)) =
        (array["name"].as_str(), array["index"].as_i64(), array["length"].as_u64())
    {
        let _ = writeln!(text, "Array: {name} · index {index} · length {length}");
    }
    let listener = &data["context"]["Listener"];
    if let (Some(signal), Some(parameter)) = (listener["signal"].as_i64(), listener["parameter"].as_i64()) {
        let _ = writeln!(text, "Listener: signal {signal} · parameter {parameter} · change {}", listener["change"]);
    }
    text
}

fn details(event: &LogEvent) -> String {
    let record = serde_json::to_string_pretty(event).unwrap_or_else(|_| "Could not format this event.".into());
    let context = script_context(&event.details, event.script_slot.is_some() || event.stage.as_deref() == Some("scripts"));
    format!("{}\n\n{context}\nComplete event record\n{record}", event.reason.as_deref().unwrap_or(&event.event))
}

fn event_heading(event: &serde_json::Value) -> String {
    format!("[{}] {} · {} · {} / {}\n{}\n",
        event["level"].as_str().unwrap_or("unknown"), time(event["timestamp_ms"].as_u64().unwrap_or(0)),
        event["path"].as_str().map(filename).or_else(|| event["library"].as_str()).unwrap_or("Application"),
        event["stage"].as_str().or_else(|| event["module"].as_str()).unwrap_or("application"),
        event["code"].as_str().or_else(|| event["event"].as_str()).unwrap_or("event"),
        event["reason"].as_str().unwrap_or(""))
}

/// Runs on the support worker; includes every retained row, never the UI filter.
fn support_text(snapshot: DiagnosticSnapshot, context: serde_json::Value) -> Result<String, String> {
    let status = &snapshot.status;
    let mut text = format!("KONTRA diagnostics — retained session report\n{}\n\nCoverage: current session retained view and available load summaries; all levels, independent of search.\nRetained {} / {} session events. Session levels: Debug {}, Info {}, Warning {}, Error {}.\nOlder events evicted from view: {}; recorder drops: {}; abbreviated events: {}; write errors: {}; retention errors: {}.\nRuntime/load-summary omission counts and cap notices are separate from recorder loss; a notice without a count has unknown omitted cardinality.\nPrevious sessions and rotated disk history are NOT included in this clipboard report. Export support report includes available retained journal history across sessions.\nPaths are redacted; filenames and diagnostic messages remain. Bounded script excerpts around faults are included. No full scripts, samples or credentials.\n\n",
        crate::build_info::SUMMARY, snapshot.events.len(), status.total_events,
        status.level_counts[0], status.level_counts[1], status.level_counts[2], status.level_counts[3],
        status.history_evicted, status.dropped_events, status.truncated_events, status.write_errors, status.retention_errors);
    let mut safe = json!({"build":snapshot.build,"status":snapshot.status,"context":context,"events":snapshot.events});
    diagnostics::clean(&mut safe, true);
    let events = safe.as_object_mut().unwrap().remove("events").unwrap();
    let mut warnings = events.as_array().unwrap().iter().filter(|event| {
        matches!(event["level"].as_str(), Some("warning" | "error"))
    }).peekable();
    text.push_str("WARNING AND ERROR DIGEST (retained events; chronological)\n");
    if warnings.peek().is_none() {
        text.push_str("No warnings or errors in the retained session view.\n");
    }
    for event in warnings {
        text.push('\n');
        text.push_str(&event_heading(event));
        text.push_str(&script_context(&event["data"], event["script_slot"].is_number() || event["stage"].as_str() == Some("scripts")));
    }
    text.push_str("\nCONFIGURATION, STATUS AND LOAD SUMMARIES\n");
    text.push_str(&serde_json::to_string_pretty(&safe).map_err(|e| e.to_string())?);
    text.push_str("\n\nALL RETAINED EVENTS (chronological; full warning/error records included)\n");
    for event in events.as_array().unwrap() {
        text.push('\n');
        text.push_str(&event_heading(event));
        text.push_str(&serde_json::to_string_pretty(event).map_err(|e| e.to_string())?);
        if let Some(excerpt) = diagnostics::excerpt_text(event) {
            text.push_str("\nScript source context (numbered lines; > marks the fault)\n");
            text.push_str(excerpt);
        }
        text.push('\n');
    }
    Ok(text)
}

fn draw(ui: &mut Ui, state: &mut State, params: &Arc<SamplerParams>) -> El {
    if let Some(text) = state.copy_ready.take() {
        ui.set_clipboard(text);
        state.copy_message = Some("Copied retained session diagnostics. Export includes older journal history.".into());
    }
    let snapshot = state.snapshot.clone();
    let status = snapshot.as_ref().map(|s| &s.status);
    let (refresh, refresh_el) = action(ui, "logs-refresh", "Refresh", false);
    if refresh {
        state.requested = None;
        state.refresh(params);
    }
    let (open, open_el) = action(ui, "logs-folder", "Open log folder", false);
    let path = status
        .and_then(|s| s.log_path.clone())
        .or_else(diagnostics::log_path);
    if open {
        state.folder_error = match path.as_deref() {
            Some(path) => if state.folder_picker.ask(picker::Ask::Reveal(path.to_path_buf())) { None }
                else { Some("Another folder operation is running. Retry when it finishes.".into()) },
            None => Some("No log folder is available yet. Refresh to retry.".into()),
        };
    }
    let (preview, export_el) = action(
        ui,
        "logs-export-preview",
        "Export support report…",
        state.preview,
    );
    if preview {
        state.preview ^= true;
        state.about = false;
        if state.destination.is_empty() {
            let root = path
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir);
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            state.destination = root
                .join(format!("support-{stamp}"))
                .to_string_lossy()
                .into_owned();
        }
    }
    let (copy, copy_el) = action(ui, "logs-copy-all", "Copy all diagnostics", false);
    if copy && state.copy_thread.is_none() {
        let params = params.clone();
        let answer = state.copy_answer.clone();
        state.copy_error = None;
        state.copy_message = Some("Preparing retained session diagnostics…".into());
        match std::thread::Builder::new().name("kontra-copy-diagnostics".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let context = params.diagnostic_report();
                support_text(diagnostics::snapshot(), context)
            })).unwrap_or_else(|_| Err("Could not collect diagnostics. Retry Copy all.".into()));
            *super::lock(&answer) = Some(result);
            WAKE.fetch_add(1, Ordering::Release);
        }) {
            Ok(handle) => state.copy_thread = Some(handle),
            Err(error) => {
                state.copy_message = None;
                state.copy_error = Some(format!("Could not start diagnostics copy: {error}"));
            }
        }
    }
    let mut content = vec![section_bar("Logs", vec![copy_el.when(state.copy_thread.is_some(), El::disabled), export_el, open_el, refresh_el])];
    if let Some(message) = &state.copy_message {
        content.push(row![caption(message.clone()).fill(secondary()).lines(2)].justify(Justify::Start).pad((INSET, TIGHT)).shrink(0));
    }
    if state.about {
        let (copy, copy_el) = action(ui, "logs-copy-build", "Copy build info", false);
        if copy {
            ui.set_clipboard(crate::build_info::SUMMARY.to_owned());
        }
        let (close, close_el) = action(ui, "logs-close-about", "Close", false);
        if close {
            state.about = false;
        }
        content.push(section_bar("About KONTRA", vec![copy_el, close_el]));
        content.push(
            body(crate::build_info::SUMMARY)
                .text_size(TEXT)
                .w(Len::Pct(100.))
                .pad(INSET)
                .shrink(0)
                .id("logs-about"),
        );
    }
    if state.about {
        content.push(spacer().flex(1));
        return col(content)
            .gap(0)
            .align(Align::Stretch)
            .flex(1)
            .min_h(0)
            .min_w(0)
            .id("logs-panel");
    }
    for error in [&state.read_error, &state.folder_error, &state.export_error, &state.copy_error]
        .into_iter()
        .flatten()
    {
        content.push(banner(Role::Warning, error.clone()));
    }
    if let Some(error) = status.and_then(|s| s.last_error.as_deref()) {
        content.push(banner(
            Role::Warning,
            format!("Log file could not be written: {error}"),
        ));
    }
    if state.preview {
        let (redact, redact_el) = check(
            ui,
            "logs-redact",
            "Redact paths in structured logs",
            state.redact,
        );
        if redact {
            state.redact ^= true;
        }
        content.push(col![
            body("Support report preview").text_size(TEXT),
            caption(crate::build_info::LABEL).fill(secondary()).tip(crate::build_info::SUMMARY).lines(2),
            caption("Build/audio settings, logs and exact archived private crash evidence. Crash originals are UNREDACTED and may contain personal paths or sensitive native fields; review before sharing.").fill(secondary()).lines(3),
            caption("Recent view: up to 2048 events / 2 MiB. Inactive history: 7 days / 64 MiB; active log rotation: up to 16 MiB.").fill(secondary()).lines(3),
            field(ui, "logs-export-path", &mut state.destination, "New report folder"),
            row![redact_el, caption(if state.redact { "Structured log paths are redacted; crash originals are unchanged" } else { "Structured log paths and private crash originals will be included" }).lines(2)].gap(SPACE).align(Align::Center),
        ].gap(TIGHT).align(Align::Stretch).pad(INSET).shrink(0).id("logs-preview"));
        let running = state.export_thread.is_some()
            || state
                .export
                .and_then(diagnostics::export_status)
                .is_some_and(|s| matches!(s, ExportStatus::Running));
        let (export, el) = action(
            ui,
            "logs-export",
            if running {
                "Exporting…"
            } else {
                "Export report"
            },
            true,
        );
        if export && state.destination.trim().is_empty() {
            state.export_error = Some("Choose a new report folder before exporting.".into());
        }
        if export && !running && !state.destination.trim().is_empty() {
            let params = params.clone();
            let destination = PathBuf::from(state.destination.trim());
            let redact = state.redact;
            let answer = state.export_answer.clone();
            state.export_error = None;
            match std::thread::Builder::new()
                .name("kontra-export-context".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut context = params.diagnostic_report();
                        context["redact_paths"] = json!(redact);
                        diagnostics::request_export(destination, context)
                    }))
                    .unwrap_or_else(|_| {
                        Err("Could not collect support context. Retry export.".into())
                    });
                    *super::lock(&answer) = Some(result);
                    WAKE.fetch_add(1, Ordering::Release);
                }) {
                Ok(handle) => state.export_thread = Some(handle),
                Err(error) => {
                    state.export_error = Some(format!("Could not start report export: {error}"))
                }
            }
        }
        content.push(
            row![el.when(running, El::disabled)]
                .pad((INSET, 0))
                .shrink(0),
        );
    }
    if state.export_thread.is_some() {
        content.push(
            caption("Preparing support context in the background…")
                .pad((INSET, TIGHT))
                .shrink(0)
                .id("logs-export-preparing"),
        );
    }
    if let Some(export) = state.export.and_then(diagnostics::export_status) {
        let mut partial = false;
        let mut warnings = String::new();
        let text = match export {
            ExportStatus::Running => "Exporting the report in the background…".to_owned(),
            ExportStatus::Complete {
                path,
                partial: incomplete,
                warnings: gaps,
            } => {
                partial = incomplete;
                warnings = gaps.join("\n");
                if incomplete {
                    format!(
                        "Report saved to {} · Partial journal history · {} coverage warnings; details in report.json and README.txt.",
                        path.display(),
                        gaps.len()
                    )
                } else {
                    format!("Report saved to {}", path.display())
                }
            }
            ExportStatus::Failed { error } => {
                format!("Export failed: {error}. Choose a new writable folder and retry.")
            }
        };
        content.push(
            body(text)
                .text_size(TEXT)
                .when(partial, |e| e.fill(Role::Warning))
                .tip(warnings)
                .lines(4)
                .pad((INSET, TIGHT))
                .shrink(0)
                .id("logs-export-status"),
        );
    }
    if state.preview {
        let (back, back_el) = action(ui, "logs-back", "Back to logs", false);
        if back {
            state.preview = false;
        }
        content.push(row![back_el].pad(INSET).shrink(0));
        content.push(spacer().flex(1));
        return col(content)
            .gap(0)
            .align(Align::Stretch)
            .flex(1)
            .min_h(0)
            .min_w(0)
            .id("logs-panel");
    }
    content.push(
        row![field(
            ui,
            "logs-search",
            &mut state.search,
            "Search diagnostics: message, library, patch, stage or load ID"
        )]
        .pad((INSET, TIGHT))
        .shrink(0),
    );
    let mut levels = Vec::new();
    for (n, level) in [
        LogLevel::Debug,
        LogLevel::Info,
        LogLevel::Warning,
        LogLevel::Error,
    ]
    .into_iter()
    .enumerate()
    {
        let count = status.map_or(0, |s| s.level_counts[n]);
        let label = format!("{} {count}", level_name(level));
        let (hit, el) = latch(
            ui,
            format!("logs-level-{n}"),
            &label,
            &format!("Show {} events", level_name(level)),
            state.levels[n],
        );
        if hit {
            state.levels[n] ^= true;
        }
        levels.push(el);
    }
    let (clear, clear_el) = action(ui, "logs-clear-filters", "Reset filters", false);
    if clear {
        state.search.clear();
        state.levels = [true; 4];
    }
    content.push(
        row![segmented(levels), spacer(), clear_el]
            .gap(SPACE)
            .pad((INSET, TIGHT))
            .shrink(0),
    );
    let Some(snapshot) = snapshot else {
        content.push(
            col![
                body(if state.read_error.is_some() {
                    "The journal is unavailable. Refresh to retry."
                } else {
                    "Reading the diagnostic journal…"
                })
                .lines(3)
            ]
            .pad(INSET)
            .flex(1)
            .min_h(0)
            .id("logs-loading"),
        );
        return col(content)
            .gap(0)
            .align(Align::Stretch)
            .flex(1)
            .min_h(0)
            .min_w(0);
    };
    let fresh = state.filter(&snapshot);
    content.push(
        row![caption(format!(
            "{} matching / {} retained (up to {}) · {} session events · {} write errors · UTC",
            state.matches.len(),
            snapshot.events.len(),
            diagnostics::HISTORY_LIMIT,
            snapshot.status.total_events,
            snapshot.status.write_errors
        ))
        .fill(secondary())
        .lines(2)].justify(Justify::Start)
        .pad((INSET, TIGHT))
        .shrink(0)
        .id("logs-count"),
    );
    if snapshot.status.history_evicted > 0
        || snapshot.status.dropped_events > 0
        || snapshot.status.truncated_events > 0
        || snapshot.status.retention_errors > 0
    {
        content.push(row![caption(format!("{} older events left the live view · {} dropped · {} oversized events abbreviated · {} retention errors. Export includes available rotated history.", snapshot.status.history_evicted, snapshot.status.dropped_events, snapshot.status.truncated_events, snapshot.status.retention_errors))
            .fill(secondary()).lines(2)].justify(Justify::Start).pad((INSET, TIGHT)).shrink(0));
    }
    content.push(rule());
    let view_h = ui
        .scene()
        .and_then(|s| s.surface("logs-list"))
        .map_or(300., |s| s.frame.size.height);
    let mut at = state.selected.and_then(|seq| {
        state
            .matches
            .iter()
            .position(|&n| snapshot.events[n].sequence == seq)
    });
    if ui
        .focus_key()
        .is_some_and(|id| id == "logs-list" || id.starts_with("log-event-"))
    {
        for key in ui.shortcuts() {
            let last = state.matches.len().saturating_sub(1);
            let page = (view_h / ROW).max(1.) as usize;
            at = match key.key {
                Key::Down => Some(at.map_or(0, |n| (n + 1).min(last))),
                Key::Up => Some(at.map_or(last, |n| n.saturating_sub(1))),
                Key::Home => Some(0),
                Key::End => Some(last),
                Key::PageDown => Some(at.map_or(0, |n| (n + page).min(last))),
                Key::PageUp => Some(at.map_or(0, |n| n.saturating_sub(page))),
                _ => continue,
            };
            if let Some(event) = at
                .and_then(|n| state.matches.get(n))
                .map(|&n| &snapshot.events[n])
            {
                state.selected = Some(event.sequence);
                state.reveal = true;
            }
        }
    }
    let from = state.y;
    if let Some(wheel) = ui.wheel("logs-list") {
        state.y += wheel.y;
    }
    let total = state.matches.len() as f64 * ROW;
    bar_drag(ui, "logs-list-bar", &mut state.y, view_h, total);
    if state.reveal {
        if let Some(at) = at {
            let top = at as f64 * ROW;
            if top < state.y {
                state.y = top;
            } else if top + ROW > state.y + view_h {
                state.y = top + ROW - view_h;
            }
        }
        state.reveal = false;
    }
    state.y = state.y.clamp(0., (total - view_h).max(0.));
    let drawn = glide(
        ui,
        "logs-list",
        state.y,
        fresh || leaps(from, state.y) || ui.get("logs-list-bar").held,
    );
    let first = (drawn / ROW) as usize;
    let last = ((drawn + view_h) / ROW).ceil() as usize;
    state.anchor = state
        .matches
        .get(first)
        .map(|&n| (snapshot.events[n].sequence, drawn - first as f64 * ROW));
    let range = first.min(state.matches.len())..last.min(state.matches.len());
    let mut items = vec![block(1, range.start as f64 * ROW).shrink(0)];
    for i in range.clone() {
        let event = &snapshot.events[state.matches[i]];
        let id = format!("log-event-{}", event.sequence);
        let title_id = format!("{id}-title");
        let reason_id = format!("{id}-reason");
        // Named text surfaces are hit targets in MUI; activation does not
        // bubble. Both text lines belong to this same selectable event row.
        if [id.as_str(), title_id.as_str(), reason_id.as_str()]
            .iter()
            .any(|target| ui.get(*target).activated())
        {
            state.selected = Some(event.sequence);
        }
        let selected = state.selected == Some(event.sequence);
        let stage = event.stage.as_deref().unwrap_or(&event.module);
        let code = event.code.as_deref().unwrap_or(&event.event);
        let patch = event.path.as_deref().map(filename)
            .or(event.library.as_deref()).unwrap_or("Application");
        let title = format!("{} · {patch} · {stage} / {code}", level_name(event.level));
        items.push(interactive(
            col![
                row![caption(title)
                    .fill(if event.level == LogLevel::Error {
                        Fill::from(Role::Warning)
                    } else {
                        secondary()
                    })
                    .lines(1)
                    .min_w(0)
                    .id(title_id)].justify(Justify::Start).w(Len::Pct(100.)).min_w(0),
                row![body(event.reason.as_deref().unwrap_or(&event.event))
                    .text_size(TEXT)
                    .lines(1)
                    .min_w(0)
                    .id(reason_id)].justify(Justify::Start).w(Len::Pct(100.)).min_w(0),
            ]
            .gap(0)
            .align(Align::Start)
            .h(ROW)
            .pad((INSET, 2.))
            .clip()
            .shrink(0)
            .when(selected, |e| e.fill(Role::Raised))
            .focusable()
            .a11y(A11y::Button)
            .named(format!(
                "{}: {}",
                level_name(event.level),
                event.reason.as_deref().unwrap_or(&event.event)
            ))
            .tip(match diagnostics::excerpt_text(&event.details) {
                Some(excerpt) => format!("{}\n\nScript source context\n{excerpt}", event.reason.as_deref().unwrap_or(&event.event)),
                None => event.reason.as_deref().unwrap_or(&event.event).to_owned(),
            })
            .id(id),
            selected,
        ));
    }
    items.push(block(1, (state.matches.len() - range.end) as f64 * ROW).shrink(0));
    if state.matches.is_empty() {
        items.push(
            body(if snapshot.events.is_empty() {
                "No diagnostic events yet. Load an instrument to record its stages."
            } else {
                "No events match these filters. Reset filters to see the retained history."
            })
            .lines(4)
            .pad(INSET)
            .shrink(0),
        );
    }
    let list = col(items)
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .h(Len::Pct(100.))
        .scroll()
        .no_scrollbar()
        .scrolled(0., drawn)
        .focusable()
        .a11y(A11y::Group)
        .named("Diagnostic events")
        .id("logs-list");
    let mut layers = vec![list];
    if total > view_h {
        layers.push(
            scrollbar(
                ui,
                "logs-list-bar",
                "Scroll diagnostic events",
                drawn,
                view_h,
                total,
            )
            .anchor(Align::End, Align::Start),
        );
    }
    content.push(stack(layers).flex(1).min_h(80.).min_w(0));
    if let Some(event) = state
        .selected
        .and_then(|seq| snapshot.events.iter().find(|e| e.sequence == seq))
    {
        let (copy, copy_el) = action(ui, "logs-copy", "Copy event details", false);
        if state
            .detail
            .as_ref()
            .is_none_or(|(seq, _)| *seq != event.sequence)
        {
            state.detail = Some((event.sequence, details(event).into()));
        }
        let full = state.detail.as_ref().unwrap().1.clone();
        if copy {
            ui.set_clipboard(full.to_string());
        }
        let (scope, scope_el) = action(ui, "logs-this-load", "This load", false);
        if scope && let Some(load) = &event.load_id {
            state.for_load(load);
        }
        content.push(rule());
        content.push(section_bar(
            "Event details",
            vec![
                scope_el.when(event.load_id.is_none(), El::disabled),
                copy_el,
            ],
        ));
        let width = ui
            .scene()
            .and_then(|s| s.surface("logs-details"))
            .map_or(550., |s| s.frame.size.width - 2. * INSET);
        content.push(
            col![
                body(full)
                    .text_size(TEXT)
                    .w(width.max(100.))
                    .shrink(0)
            ]
            .pad(INSET)
            .h(if diagnostics::excerpt_text(&event.details).is_some() { 200. } else { 120. })
            .shrink(0)
            .scroll()
            .id("logs-details"),
        );
    } else {
        content.push(rule());
        content.push(section_bar("Event details", Vec::new()));
        content.push(
            col![
                caption("Select an event to inspect its complete record or copy it.")
                    .fill(secondary())
                    .lines(3)
            ]
            .pad(INSET)
            .h(120.)
            .shrink(0)
            .id("logs-details"),
        );
    }
    col(content)
        .gap(0)
        .align(Align::Stretch)
        .flex(1)
        .min_h(0)
        .min_w(0)
        .id("logs-panel")
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Board(Arc<Mutex<String>>);
    impl moose::mui::mui::Clipboard for Board {
        fn get(&mut self) -> Option<String> { Some(super::super::lock(&self.0).clone()) }
        fn set(&mut self, text: &str) { *super::super::lock(&self.0) = text.to_owned(); }
    }

    fn tick(ui: &mut Ui, state: &mut State, params: &Arc<SamplerParams>, input: Input) {
        let root = draw(ui, state, params).w(900.).h(700.);
        ui.frame(root, Some(Size::new(900., 700.)), input, 1. / 60.)
            .unwrap();
    }
    fn press(ui: &mut Ui, state: &mut State, params: &Arc<SamplerParams>, id: &str) {
        ui.focus(id);
        tick(
            ui,
            state,
            params,
            Input {
                keys: vec![KeyPress {
                    key: Key::Enter,
                    mods: Mods::default(),
                }],
                ..Default::default()
            },
        );
        tick(ui, state, params, Input::default());
        tick(ui, state, params, Input::default());
    }
    fn type_into(
        ui: &mut Ui,
        state: &mut State,
        params: &Arc<SamplerParams>,
        id: &str,
        value: &str,
    ) {
        ui.focus(id);
        tick(
            ui,
            state,
            params,
            Input {
                text: value.into(),
                ..Default::default()
            },
        );
        tick(ui, state, params, Input::default());
        tick(ui, state, params, Input::default());
    }

    fn wait_export(ui: &mut Ui, state: &mut State, params: &Arc<SamplerParams>, previous: Option<u64>) -> ExportStatus {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            state.refresh(params);
            tick(ui, state, params, Input::default());
            if state.export_thread.is_none() {
                if let Some(error) = &state.export_error {
                    panic!("Could not start export: {error}");
                }
                // Completion can arrive after draw has disabled the button for
                // Running. Wait for the completed request's enabled scene too,
                // so the next activation is not sent to that stale scene.
                if ui.scene().and_then(|scene| scene.surface("logs-export")).is_some_and(|surface| !surface.disabled)
                    && let Some(status) = state.export.filter(|id| Some(*id) != previous).and_then(diagnostics::export_status)
                    && !matches!(status, ExportStatus::Running)
                {
                    return status;
                }
            }
            assert!(
                std::time::Instant::now() < until,
                "support export did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn the_global_log_panel_filters_and_virtualizes_retained_history() {
        let params = Arc::new(SamplerParams::new());
        let clipboard = Arc::new(Mutex::new(String::new()));
        let mut ui = super::super::theme::ui().clipboard(Board(clipboard.clone()));
        let events: Vec<_> = (0..2048)
            .map(|n| LogEvent {
                schema_version: 1,
                instance_id: None,
                script_epoch: None,
                outcome: None,
                sequence: n + 1,
                timestamp_ms: 1_759_392_000_000 + n * 100,
                monotonic_ms: n * 100,
                session_id: "synthetic-ui-fixture".into(),
                level: [
                    LogLevel::Debug,
                    LogLevel::Info,
                    LogLevel::Warning,
                    LogLevel::Error,
                ][n as usize % 4],
                module: "loader".into(),
                event: "issue".into(),
                stage: Some("samples".into()),
                code: Some("resolved_reference".into()),
                load_id: Some((n / 64).to_string()),
                path: Some(if n == 1001 {
                    r"C:\virtual\Fixture Strings\Instruments\Violin.nki".into()
                } else { format!(
                    "/virtual/{}/Instruments/{}.nki",
                    if n % 2 == 0 {
                        "Fixture Keys"
                    } else {
                        "Fixture Strings"
                    },
                    if n % 2 == 0 { "Cello" } else { "Violin" }
                ) }),
                // Real loader events can carry a path before catalog identification.
                library: if n == 1001 {
                    None
                } else {
                    Some(if n % 2 == 0 {
                        "Fixture Keys".into()
                    } else {
                        "Fixture Strings".into()
                    })
                },
                program: Some(0),
                part: Some(0),
                script_slot: Some(0),
                line: Some(42),
                reason: Some(format!("Marker {n}: sample reference resolved.")),
                details: if n == 1001 {
                    json!({"reason":"synthetic test event","line":42,
                        "array":{"name":"%bad","index":3,"length":2},
                        "last_action":{"callback":"note","callback_id":7,"event_id":11,"note":62,"velocity":90,"midi_channel":2},
                        "source_excerpt":diagnostics::script_excerpt(
                        &format!("{}malformed(\"context)\nend on", "\n".repeat(41)),1,42,Some(11))})
                } else if n == 1003 {
                    json!({"context":{"MidiNote":{"builtin":"set_key_color","argument":1,"value":128}},
                        "source_excerpt_unavailable":"Cached script source or reported line is unavailable",
                        "last_action":{"callback":"ui_control","callback_id":8,"ui_control":1},
                        "access_key":"private-token"})
                } else { json!({"reason":"synthetic test event","line":42}) },
            })
            .collect();
        let snapshot = DiagnosticSnapshot {
            // Copy/export workers later install real journal snapshots; do not
            // give this unrelated synthetic history a live journal revision.
            revision: u64::MAX,
            events,
            status: diagnostics::LogStatus {
                total_events: 10_000,
                level_counts: [2500; 4],
                history_evicted: 7952,
                log_path: Some("/virtual/logs/session.jsonl".into()),
                ..Default::default()
            },
            build: json!({"fixture":true}),
        };
        let mut state = State::default();
        state.snapshot = Some(Arc::new(snapshot));
        for _ in 0..3 {
            tick(&mut ui, &mut state, &params, Input::default());
        }
        assert_eq!(
            state.matches.len(),
            1024,
            "warnings and errors show first; catalog progress remains available by severity"
        );
        let mounted = (1..=2048)
            .filter(|n| {
                ui.scene()
                    .unwrap()
                    .surface(&format!("log-event-{n}"))
                    .is_some()
            })
            .count();
        assert!(
            mounted > 0 && mounted < 64,
            "only the viewport's rows mount: {mounted}"
        );
        ui.focus("logs-list");
        tick(
            &mut ui,
            &mut state,
            &params,
            Input {
                keys: vec![KeyPress {
                    key: Key::End,
                    mods: Mods::default(),
                }],
                ..Default::default()
            },
        );
        for _ in 0..3 {
            tick(&mut ui, &mut state, &params, Input::default());
        }
        assert_eq!(
            state.selected,
            Some(3),
            "End reaches the oldest matching event"
        );
        assert!(ui.scene().unwrap().surface("log-event-3").is_some(),
            "oldest selected event remains in the viewport: list={:?}, y={}, matches={}",
            ui.scene().unwrap().surface("logs-list").map(|s| s.frame), state.y, state.matches.len());
        press(&mut ui, &mut state, &params, "logs-level-1");
        type_into(&mut ui, &mut state, &params, "logs-search", "Marker 1001 Fixture Strings Violin samples load:15");
        assert_eq!(
            state.matches.len(),
            1,
            "one query intersects message, inferred library path, patch, stage and exact load"
        );
        for removed in ["logs-library", "logs-patch", "logs-load"] {
            assert!(ui.scene().unwrap().surface(removed).is_none(), "only one search input remains");
        }
        // Exercise real pointer hits, not only keyboard activation of the row.
        // Named title/reason surfaces must select exactly the same event as
        // the blank area at the right of its parent row.
        for target in ["title", "reason", "blank"] {
            state.selected = None;
            tick(&mut ui, &mut state, &params, Input::default());
            let scene = ui.scene().unwrap();
            let row = scene.surface("log-event-1002").unwrap().frame;
            let at = if target == "blank" {
                Point::new(row.x + row.size.width - INSET / 2., row.y + row.size.height / 2.)
            } else {
                let text = scene.surface(&format!("log-event-1002-{target}")).unwrap().frame;
                Point::new(text.x + text.size.width / 2., text.y + text.size.height / 2.)
            };
            for down in [true, false] {
                tick(&mut ui, &mut state, &params, Input {
                    pointer: PointerInput {
                        pos: Some(at),
                        buttons: if down { Buttons::PRIMARY } else { Buttons::default() },
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
            tick(&mut ui, &mut state, &params, Input::default());
            assert_eq!(state.selected, Some(1002), "clicking event {target} selects the whole row");
        }
        press(&mut ui, &mut state, &params, "log-event-1002");
        assert_eq!(state.selected, Some(1002));
        assert!(ui.scene().unwrap().surface("logs-copy").is_some());
        assert!(state.detail.as_ref().unwrap().1.contains("Marker 1001"));
        assert!(state.detail.as_ref().unwrap().1.contains("\n>     42 | malformed(\"context)\n"), "selected details display actual numbered code, not only JSON escapes");
        let detail = &state.detail.as_ref().unwrap().1;
        assert!(detail.contains("Callback: note · callback ID 7 · event ID 11 · note 62 · velocity 90 · MIDI channel 3"));
        assert!(detail.contains("Array: %bad · index 3 · length 2"));
        assert!(detail.find("Script source context").unwrap() < detail.find("Complete event record").unwrap());
        assert_eq!(filename(r"C:\private-user\Instruments\Violin.nki"), "Violin.nki");
        let unavailable = details(&state.snapshot.as_ref().unwrap().events[1003]);
        assert!(unavailable.contains("Argument: set_key_color · argument 1 · value 128"));
        assert!(unavailable.contains("Callback: ui_control · callback ID 8 · UI control 1"));
        assert!(unavailable.contains("Script source context unavailable: Cached script source or reported line is unavailable."));
        assert!(!unavailable.contains("numbered lines"), "missing code is never reconstructed from a diagnostic message");
        assert!(details(&state.snapshot.as_ref().unwrap().events[1000]).contains("no excerpt was retained"));
        let scene = ui.scene().unwrap();
        let row = scene.surface("log-event-1002").unwrap().frame;
        let title = scene.surface("log-event-1002-title").unwrap().frame;
        let reason = scene.surface("log-event-1002-reason").unwrap().frame;
        let font = Font::new(NOTO_SANS).unwrap();
        let caption_pitch = ui.text_run(&font, "M", SMALL).unwrap().line_height;
        let body_pitch = ui.text_run(&font, "M", TEXT).unwrap().line_height;
        assert!(
            reason.y + reason.size.height <= row.y + row.size.height - 2.,
            "both compact lines fit above the bottom inset: row={row:?}, \
             title={title:?}, reason={reason:?}, \
             caption_pitch={caption_pitch}, body_pitch={body_pitch}, \
             required_span={}",
            caption_pitch + body_pitch + 4.,
        );
        assert!((title.x - row.x - INSET).abs() <= 1. && (reason.x - title.x).abs() <= 1., "row text starts at the left inset: {row:?}, {title:?}, {reason:?}");
        let copied = support_text(state.snapshot.as_ref().unwrap().as_ref().clone(), json!({
            "parts":[{"path":"/virtual/private-user/Diagnostic Test.nki","load":{"issues_omitted":7,"notes":["runtime diagnostic cap reached; omitted locations unknown"]}}],
            "script_source":"private script payload"})).unwrap();
        assert!(copied.contains("Marker 0:") && copied.contains("Marker 2047:"), "Copy all includes every retained severity, independent of this single-row search");
        assert!(copied.contains("7952") && copied.contains("10000") && copied.contains("issues_omitted") && copied.contains("omitted locations unknown"), "recorder coverage and load/runtime omissions remain distinct");
        assert!(copied.contains("Diagnostic Test.nki") && !copied.contains("/virtual/private-user") && !copied.contains("private script payload"), "default redaction keeps filenames and removes private payloads");
        assert!(copied.contains("Previous sessions and rotated disk history are NOT included"));
        let digest = copied.find("WARNING AND ERROR DIGEST").unwrap();
        let configuration = copied.find("CONFIGURATION, STATUS AND LOAD SUMMARIES").unwrap();
        assert!(digest < configuration);
        let digest = &copied[digest..configuration];
        assert!(digest.contains("Marker 1003:") && digest.contains("Argument: set_key_color · argument 1 · value 128"));
        assert!(digest.contains("Script source context unavailable") && !digest.contains("Marker 0:"), "digest shows actionable severities and honest source coverage");
        assert!(!copied.contains("private-token") && !copied.contains(r"C:\virtual"), "digest uses the same sanitized records as the complete report");
        assert!(copied.contains("\"last_action\"") && copied.contains("\"MidiNote\""), "readable context supplements the original structured records");
        assert!(copied.contains("\n>     42 | malformed(\"context)\n"), "Copy all includes readable source context alongside its structured record");
        let shot = Path::new("artifacts/diagnostics/log-panel-fixture.png");
        std::fs::create_dir_all(shot.parent().unwrap()).unwrap();
        moose::core::screenshot::save_png(
            shot,
            &super::super::tests::pixels(&ui, 900, 700),
            900,
            700,
        );
        state.about = true;
        tick(&mut ui, &mut state, &params, Input::default());
        let about = ui.scene().unwrap().surface("logs-about").unwrap().frame;
        assert!(about.size.height >= crate::build_info::SUMMARY.lines().count() as f64 * body_pitch + 2. * INSET - 1.,
            "all build fields must fit, including target/profile/features beyond the fifth line: {about:?}");
        assert!(about.size.width >= 850., "About uses the offered Logs width: {about:?}");
        let path = Path::new("artifacts/diagnostics/log-about-fixture.png");
        moose::core::screenshot::save_png(path, &super::super::tests::pixels(&ui, 900, 700), 900, 700);
        press(&mut ui, &mut state, &params, "logs-close-about");
        assert!(!state.about);
        press(&mut ui, &mut state, &params, "logs-export-preview");
        assert!(
            state.preview && state.redact,
            "preview opens with path redaction on"
        );
        assert!(ui.scene().unwrap().surface("logs-export-path").is_some());
        let path = Path::new("artifacts/diagnostics/log-export-preview-fixture.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        moose::core::screenshot::save_png(
            path,
            &super::super::tests::pixels(&ui, 900, 700),
            900,
            700,
        );
        press(&mut ui, &mut state, &params, "logs-export-preview");
        press(&mut ui, &mut state, &params, "logs-clear-filters");
        assert_eq!(
            state.matches.len(),
            2048,
            "reset restores every retained event"
        );
        press(&mut ui, &mut state, &params, "logs-level-3");
        assert_eq!(
            state.matches.len(),
            1536,
            "severity toggles affect the actual list"
        );
        assert!(
            !state.filter(state.snapshot.clone().as_ref().unwrap()),
            "unchanged filters reuse the index"
        );

        press(&mut ui, &mut state, &params, "logs-copy-all");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while state.copy_thread.is_some() {
            state.refresh(&params);
            tick(&mut ui, &mut state, &params, Input::default());
            assert!(std::time::Instant::now() < until, "Copy all diagnostics worker did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(state.copy_error.is_none(), "{:?}", state.copy_error);
        let copied = super::super::lock(&clipboard).clone();
        assert!(copied.starts_with("KONTRA diagnostics") && copied.contains(crate::build_info::SUMMARY) && copied.contains("CONFIGURATION, STATUS AND LOAD SUMMARIES"), "the actual Copy all action publishes worker report text to the clipboard");

        // The actual export handoff also works with a selected path but no loaded bank.
        params
            .selection
            .write()
            .unwrap()
            .parts
            .push(crate::plugin::Part {
                path: "/virtual/private-user/Diagnostic Test.nki".into(),
                ..Default::default()
            });
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let destination =
            std::env::temp_dir().join(format!("kontra-log-ui-{}-{stamp}", std::process::id()));
        state.destination = destination.to_string_lossy().into_owned();
        press(&mut ui, &mut state, &params, "logs-export-preview");
        let previous = state.export;
        press(&mut ui, &mut state, &params, "logs-export");
        let ExportStatus::Complete { path, .. } = wait_export(&mut ui, &mut state, &params, previous) else {
            panic!("support report should export")
        };
        assert_eq!(path, destination);
        for file in ["report.json", "events.jsonl", "journal.jsonl", "README.txt"] {
            assert!(destination.join(file).is_file(), "missing {file}");
        }
        let report_before = std::fs::read(destination.join("report.json")).unwrap();
        assert!(
            !String::from_utf8_lossy(&report_before).contains("/virtual/private-user"),
            "default export redacts local paths"
        );
        assert!(ui.scene().unwrap().surface("logs-export-status").is_some());
        let previous = state.export;
        press(&mut ui, &mut state, &params, "logs-export");
        assert!(state.export_thread.is_some(), "the re-enabled export button accepts a new request");
        assert!(
            matches!(
                wait_export(&mut ui, &mut state, &params, previous),
                ExportStatus::Failed { .. }
            ),
            "an existing destination fails without overwrite"
        );
        assert_eq!(
            std::fs::read(destination.join("report.json")).unwrap(),
            report_before
        );
        std::fs::remove_dir_all(destination).unwrap();
    }
}
