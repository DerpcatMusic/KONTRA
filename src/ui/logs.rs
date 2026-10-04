//! Global loader/runtime journal. Snapshot copies and report export stay off the UI thread.
use super::{Cx, picker, theme::*};
use crate::diagnostics::{self, DiagnosticSnapshot, ExportStatus, LogEvent, LogLevel};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use serde_json::json;
use std::collections::{HashMap, HashSet};
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

struct ReportReceipt {
    status: String,
    issue_url: Option<String>,
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
    receipt_snapshot: Option<Arc<DiagnosticSnapshot>>,
    receipt: Option<ReportReceipt>,
    requested: Option<u64>,
    read_error: Option<String>,
    search: String,
    levels: [bool; 4],
    filtered: Option<(u64, Option<PathBuf>, usize, String, [bool; 4])>,
    matches: Vec<usize>,
    groups: Vec<EventGroup>,
    matching_events: usize,
    raw_details: bool,
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
            receipt_snapshot: None,
            receipt: None,
            requested: None,
            read_error: None,
            search: String::new(),
            levels: [false, false, true, true],
            filtered: None,
            matches: Vec::new(),
            groups: Vec::new(),
            matching_events: 0,
            raw_details: false,
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
    fn cache_receipt(&mut self, snapshot: &Arc<DiagnosticSnapshot>) {
        if self
            .receipt_snapshot
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, snapshot))
        {
            return;
        }
        // Receipt visibility is independent of log filters. Scan only when the
        // background reader replaces its immutable snapshot, never every frame.
        self.receipt = snapshot.events.iter().rev().find_map(|event| {
            if event.module != "support"
                || !matches!(event.event.as_str(), "automatic_crash_report" | "previous_crash_report")
            {
                return None;
            }
            let sent = event.details["sent"].as_bool()?;
            let report = event.details["report_id"].as_str().filter(|id| {
                !id.is_empty() && id.len() <= 128
                    && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
            let status = if sent {
                report.map_or_else(|| "Crash report sent.".into(), |id| format!("Crash report sent. Report ID: {id}"))
            } else if event.details["manual_export_required"] == true {
                "Crash report requires manual export. Automatic retry is paused for unchanged evidence.".into()
            } else {
                "Crash report retained for retry. See the support log for details.".into()
            };
            let issue_url = sent.then(|| event.details["issue_url"].as_str()
                .and_then(crate::support::public_issue_url)).flatten();
            Some(ReportReceipt { status, issue_url })
        });
        self.receipt_snapshot = Some(snapshot.clone());
    }

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
            snapshot.status.log_path.clone(),
            snapshot.events.len(),
            self.search.clone(),
            self.levels,
        );
        if self.filtered.as_ref() == Some(&key) {
            return false;
        }
        let needle = self.search.to_lowercase();
        let words: Vec<_> = needle.split_whitespace().collect();
        let criteria_changed = self.filtered.as_ref().is_none_or(|old| {
            (&old.3, old.4) != (&key.3, key.4)
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
        self.matching_events = self.matches.len();
        self.groups = group_events(&snapshot.events, &self.matches);
        self.matches = self.groups.iter().map(|group| group.latest).collect();
        self.detail = None;
        if let Some(selected) = self.selected {
            self.selected = self
                .groups
                .iter()
                .find(|group| {
                    group
                        .members
                        .iter()
                        .any(|&n| snapshot.events[n].sequence == selected)
                })
                .map(|group| snapshot.events[group.latest].sequence);
        }
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

#[cfg(feature = "uvi")]
fn local_lua_context(params:&SamplerParams,event:&LogEvent)->Option<Arc<crate::uvi::lua_failure::Context>> {
    let data=&event.details;
    let slot=usize::try_from(data["slot"].as_u64()?).ok()?;
    let epoch=data["epoch"].as_u64()?;let generation=data["generation"].as_u64()?;
    if epoch!=params.shared.uvi_activation_epoch() {return None;}
    let view=params.shared.view.lock().ok()?;
    let part=view.parts.get(slot)?;
    let context=part.local_lua_failure(std::path::Path::new(event.path.as_deref()?),
        data["member"].as_str()?,epoch,generation)?;
    let retained=&data["worker"]["lua_failure"];
    if retained["processor"].as_u64()!=context.processor.map(|p|p as u64)
        || retained["frame"].as_u64()!=Some(context.frame)
        || retained["line"].as_u64()!=context.line.map(u64::from)
        || retained["chunk"].as_str()!=Some(context.chunk.as_str()) {return None;}
    Some(context)
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

// Group only retained matching events. Raw journal/export records stay untouched.
struct EventGroup {
    latest: usize,
    members: Vec<usize>,
    first_ms: u64,
    last_ms: u64,
    children: Vec<Vec<usize>>,
}

fn cause(event: &LogEvent) -> String {
    // The worker's original failure outranks the typed endpoint symptom.
    [event.details.pointer("/worker/failure"), event.details.get("cause"),
        event.details.get("failure"), event.details.get("worker_failure"),
        event.details.pointer("/endpoint/error"), event.details.get("error")]
        .into_iter().flatten().find(|value| !value.is_null())
        .map(|value| {
            let value = stable_cause(value);
            value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())
        })
        .unwrap_or_else(|| event.reason.clone().unwrap_or_else(|| event.event.clone()))
}

fn stable_cause(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(fields) => Value::Object(fields.iter().filter(|(key, _)|
            !matches!(key.as_str(), "timestamp" | "timestamp_ms" | "monotonic_ms" | "frame" | "processed_frame" |
                "epoch" | "generation" | "block" | "stats" | "count" | "first_ms" | "last_ms"))
            .map(|(key, value)| (key.clone(), stable_cause(value))).collect()),
        Value::Array(items) => Value::Array(items.iter().map(stable_cause).collect()),
        _ => value.clone(),
    }
}

fn event_stage(event: &LogEvent) -> &str {
    event.stage.as_deref().or_else(|| event.details["endpoint"]["stage"].as_str()).unwrap_or(&event.module)
}

fn inventory(event: &LogEvent) -> bool {
    event.code.as_deref() == Some("load_detail_inventory") || event.details["code"] == "load_detail_inventory"
}

fn inventory_field(event: &LogEvent) -> String {
    event.details["field"].as_array().map(|field| field.iter().filter_map(|part| part.as_str()).collect::<Vec<_>>().join("."))
        .or_else(|| event.details["field"].as_str().map(str::to_owned)).unwrap_or_else(|| "unknown field".into())
}

fn inventory_summary(events: &[LogEvent], group: &EventGroup) -> String {
    let latest = &events[group.latest];
    let total = latest.details["items_total"].as_u64();
    let mut chunks = HashSet::new();
    let mut expected_chunks = None;
    let mut ranges = Vec::new();
    for &n in &group.members {
        let data = &events[n].details;
        if let Some(chunk) = data["chunk"].as_u64() {
            chunks.insert(chunk);
            if data["final"] == true { expected_chunks = Some(chunk.saturating_add(1)); }
        }
        if let (Some(start), Some(items)) = (data["item_start"].as_u64(), data["items"].as_array()) {
            ranges.push((start, start.saturating_add(items.len() as u64)));
        }
    }
    // Count overlapping/repeated chunks once; a retained subset never claims complete inventory.
    ranges.sort_unstable();
    let (mut rows, mut end) = (0u64, 0u64);
    for (start, stop) in ranges {
        rows += stop.saturating_sub(start.max(end));
        end = end.max(stop);
    }
    format!("{} · {} / {} rows in matching retained chunks · {} / {} chunks · revision {}",
        inventory_field(latest), rows, total.map_or_else(|| "unknown total".into(), |n| n.to_string()),
        chunks.len(), expected_chunks.map_or_else(|| "unknown total".into(), |n| n.to_string()),
        latest.details["revision"])
}

fn group_events(events: &[LogEvent], indices: &[usize]) -> Vec<EventGroup> {
    let mut groups: Vec<EventGroup> = Vec::new();
    let mut parents = HashMap::new();
    let mut children: Vec<HashMap<String, usize>> = Vec::new();
    for &n in indices {
        let event = &events[n];
        // Information/progress events remain individually selectable.
        let key = if inventory(event) && event.load_id.is_some() {
            format!("inventory:{}", json!([event.session_id, level_index(event.level), event.module,
                event.load_id, event.details["field"], event.details["revision"]]))
        } else if matches!(event.level, LogLevel::Warning | LogLevel::Error) {
            json!([
                event.session_id,
                level_index(event.level),
                event.module,
                event_stage(event),
                event.code.as_deref().unwrap_or(&event.event)
            ])
            .to_string()
        } else {
            format!("record:{n}")
        };
        let at = *parents.entry(key).or_insert_with(|| {
            let at = groups.len();
            groups.push(EventGroup {
                latest: n,
                members: Vec::new(),
                first_ms: event.timestamp_ms,
                last_ms: event.timestamp_ms,
                children: Vec::new(),
            });
            children.push(HashMap::new());
            at
        });
        let group = &mut groups[at];
        if event.sequence > events[group.latest].sequence {
            group.latest = n;
        }
        group.first_ms = group.first_ms.min(event.timestamp_ms);
        group.last_ms = group.last_ms.max(event.timestamp_ms);
        group.members.push(n);
        if inventory(event) {
            if group.children.is_empty() { group.children.push(Vec::new()); }
            group.children[0].push(n);
            continue;
        }
        let identity: Vec<_> = [
            "slot",
            "bank",
            "member",
            "preset",
            "processor",
            "processor_id",
            "node_id",
            "location",
            "file",
            "source_file",
            "source_line",
            "line",
            "column",
            "function",
            "cause",
            "failure",
            "error",
            "worker_failure",
        ]
        .into_iter()
        .filter_map(|key| event.details.get(key).map(|value| (key, stable_cause(value))))
        .collect();
        let child_key = json!([
            event.path,
            event.library,
            event.program,
            event.part,
            event.script_slot,
            event.line,
            identity,
            cause(event),
            event.details.pointer("/endpoint/error_code"),
            event.details.pointer("/endpoint/stage"),
            event.details.pointer("/endpoint/source_file").or_else(|| event.details.pointer("/endpoint/source")),
            event.details.pointer("/endpoint/line")
        ])
        .to_string();
        let child = *children[at].entry(child_key).or_insert_with(|| {
            group.children.push(Vec::new());
            group.children.len() - 1
        });
        group.children[child].push(n);
    }
    groups
}

fn item_label(event: &LogEvent) -> String {
    let mut labels = Vec::new();
    if let Some(path) = event.path.as_deref().filter(|path| !path.is_empty()) {
        labels.push(path.to_owned());
    } else if let Some(library) = &event.library {
        labels.push(library.clone());
    }
    for key in [
        "bank",
        "member",
        "preset",
        "processor",
        "processor_id",
        "node_id",
        "location",
        "file",
        "source_file",
        "source_line",
        "column",
        "function",
    ] {
        if let Some(value) = event.details.get(key).filter(|value| !value.is_null()) {
            labels.push(format!(
                "{key}: {}",
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            ));
        }
    }
    if let Some(endpoint) = event.details.get("endpoint") {
        for key in ["source_file", "source", "line", "stage", "error_code"] {
            if let Some(value) = endpoint.get(key).filter(|value| !value.is_null()) {
                labels.push(format!("{key}: {}", value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())));
            }
        }
    }
    if let Some(slot) = event.part.map(|n| n as u64).or_else(|| event.details["slot"].as_u64()).or_else(|| event.details["part"].as_u64()) {
        labels.push(format!("rack slot {}", slot + 1));
    }
    if let Some(program) = event.program {
        labels.push(format!("program {program}"));
    }
    if let Some(slot) = event.script_slot.filter(|_| event.module != "uvi") {
        labels.push(format!("script {slot}"));
    }
    if let Some(line) = event.line {
        labels.push(format!("line {line}"));
    }
    if labels.is_empty() {
        "Application / item location not retained".into()
    } else {
        labels.join(" · ")
    }
}

fn retained_excerpt(event: &LogEvent) -> Option<&str> {
    diagnostics::excerpt_text(&event.details)
        .or_else(|| diagnostics::excerpt_text(&event.details["endpoint"]))
        .or_else(|| diagnostics::excerpt_text(&event.details["worker"]))
}

fn group_reason(events: &[LogEvent], group: &EventGroup) -> String {
    let latest = &events[group.latest];
    if inventory(latest) { return inventory_summary(events, group); }
    let first_cause = cause(latest);
    if group.children.iter().all(|child| cause(&events[child[0]]) == first_cause) {
        first_cause
    } else {
        format!("{} unique item/cause/location entries · select to inspect", group.children.len())
    }
}

fn group_details(events: &[LogEvent], group: &EventGroup) -> String {
    use std::fmt::Write;
    let event = &events[group.latest];
    if inventory(event) {
        return format!("Inventory for load {}\n{}\n{} matching retained records · First: {} · Last: {}\n\nInventory rows are preserved in the raw journal and full support export. Latest raw record shows one retained chunk. Search/filter counts cover matching retained chunks only.\n",
            event.load_id.as_deref().unwrap_or("unknown"), inventory_summary(events, group),
            group.members.len(), time(group.first_ms), time(group.last_ms));
    }
    let mut text = format!(
        "{} / {} — {} occurrences, {} unique items or locations/causes\nFirst: {} · Last: {} (retained matching events)\n",
        event_stage(event),
        event.code.as_deref().unwrap_or(&event.event),
        group.members.len(),
        group.children.len(),
        time(group.first_ms),
        time(group.last_ms)
    );
    for child in group.children.iter().take(64) {
        let latest = *child.iter().max_by_key(|&&n| events[n].sequence).unwrap();
        let source = child.iter().filter(|&&n| retained_excerpt(&events[n]).is_some())
            .max_by_key(|&&n| events[n].sequence).copied().unwrap_or(latest);
        let event = &events[latest];
        let first = child.iter().map(|&n| events[n].timestamp_ms).min().unwrap();
        let last = child.iter().map(|&n| events[n].timestamp_ms).max().unwrap();
        let _ = writeln!(
            text,
            "\n  {} — {} occurrences · {}–{}\nCause: {}",
            item_label(event),
            child.len(),
            time(first),
            time(last),
            cause(event)
        );
        let source_data = &events[source].details;
        let source_context = if diagnostics::excerpt_text(source_data).is_some() { source_data }
            else if diagnostics::excerpt_text(&source_data["endpoint"]).is_some() || source_data["endpoint"]["source_excerpt_unavailable"].is_string() { &source_data["endpoint"] }
            else if diagnostics::excerpt_text(&source_data["worker"]).is_some() || source_data["worker"]["source_excerpt_unavailable"].is_string() { &source_data["worker"] }
            else { source_data };
        let context = script_context(source_context, event.module != "uvi" &&
            (event.script_slot.is_some() || event.stage.as_deref() == Some("scripts")));
        let label = if source_context["source_kind"] == "rust" || source_context["source_excerpt"]["source_kind"] == "rust" {
            "Rust source context"
        } else { "Source context" };
        text.push_str(&if event.module == "uvi" { context.replace("Script source context", label) } else { context });
        // Cause and real source context are already shown above; avoid repeating them in JSON.
        let mut diagnostic = event.details.clone();
        if let Some(object) = diagnostic.as_object_mut() {
            for key in ["slot", "part", "bank", "member", "preset", "processor", "processor_id", "node_id",
                "location", "file", "source_file", "source_line", "line", "column", "function"] { object.remove(key); }
        }
        for (parent, keys) in [("worker", &["failure", "source_excerpt"][..]),
            ("endpoint", &["error", "source_excerpt", "source", "source_file", "line", "stage", "error_code"][..])] {
            if let Some(object) = diagnostic[parent].as_object_mut() {
                for &key in keys { object.remove(key); }
            }
        }
        let diagnostic = compact_context(&diagnostic, 0);
        if text.len() > 64 * 1024 {
            let _ = writeln!(
                text,
                "Remaining item details omitted at 64 KiB; {} unique items total. Full support export preserves records.",
                group.children.len()
            );
            break;
        }
        if diagnostic != json!({}) {
            let record = serde_json::to_string_pretty(&diagnostic).unwrap_or_default();
            if record.len() <= 4096 {
                let _ = writeln!(text, "{record}");
            } else {
                text.push_str("Additional diagnostic fields available in the raw record and full support export.\n");
            }
        }
    }
    if group.children.len() > 64 {
        let _ = writeln!(
            text,
            "{} more unique items/locations available in full support export.",
            group.children.len() - 64
        );
    }
    text
}

fn compact_context(value: &serde_json::Value, depth: usize) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter_map(|(key, value)| {
                    if matches!(
                        key.as_str(),
                        "source_excerpt"
                            | "reason"
                            | "cause"
                            | "failure"
                            | "worker_failure"
                            | "error"
                    ) && depth == 0
                    {
                        return None;
                    }
                    if matches!(
                        key.as_str(),
                        "nodes"
                            | "held_keys"
                            | "heard"
                            | "played"
                            | "output_buses"
                            | "sustain_cc"
                            | "sostenuto_cc"
                    ) {
                        return Some((
                            format!("{key}_items_in_full_export"),
                            json!(value.as_array().map_or(0, Vec::len)),
                        ));
                    }
                    if value.is_null() {
                        return None;
                    }
                    Some((key.clone(), compact_context(value, depth + 1)))
                })
                .collect(),
        ),
        Value::Array(items) if items.len() > 16 || depth > 8 => {
            json!({"items_in_full_export":items.len()})
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| compact_context(item, depth + 1))
                .collect(),
        ),
        Value::String(text) if text.len() > 1024 => {
            let end = text
                .char_indices()
                .map(|(n, _)| n)
                .take_while(|&n| n <= 1024)
                .last()
                .unwrap_or(0);
            json!(format!("{}… [complete text in full export]", &text[..end]))
        }
        _ => value.clone(),
    }
}

// Human support copy needs current operating state, not inactive rack slots or inventories.
fn support_context(context: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    fn fields(value: &Value, keys: &[&str]) -> Value {
        Value::Object(keys.iter().filter_map(|&key| value.get(key).filter(|v| !v.is_null())
            .map(|v| (key.into(), v.clone()))).collect())
    }
    let mut summary = fields(context, &["instance_id", "memory", "audio_snapshots_dropped", "log_flush_error", "parts"]);
    if let Some(host) = context.get("host") {
        summary["host"] = fields(host, &["sample_rate"]);
        summary["host"]["audio"] = fields(&host["audio"], &["sample_rate", "block_size", "output_channels", "offline",
            "alignment_overflows", "host_note_end_rejections", "unsupported_host_expression"]);
    }
    if let Some(rack) = context.get("rack") {
        summary["rack"] = fields(rack, &["outputs", "auto_align", "midi_thru"]);
        if let Some(parts) = rack["parts"].as_array() {
            summary["rack"]["parts"] = Value::Array(parts.iter().map(|part| {
                let mut state = fields(part, &["slot", "path", "program", "name", "status", "runtime_status", "generation", "script_epoch"]);
                if !part["load"].is_null() {
                    state["load"] = fields(&part["load"], &["status", "duration_ms", "issues_omitted", "notes", "terminal_failure"]);
                }
                state
            }).collect());
        }
    }
    if let Some(uvi) = context.get("uvi") {
        summary["uvi"] = fields(uvi, &["live_installed", "block_frames", "queue_capacity", "requested", "node_report_coverage"]);
        if let Some(workers) = uvi["rack_workers"].as_array() {
            summary["uvi"]["rack_workers"] = Value::Array(workers.iter().map(|worker| {
                let mut state = fields(worker, &["slot", "epoch", "generation", "cancelled", "endpoint_retired", "endpoint_exported", "sample_rate", "max_host_frames"]);
                state["worker"] = fields(&worker["worker"], &["status", "phase", "frame", "failure", "stats"]);
                state["worker"]["program"] = fields(&worker["worker"]["program"], &["counts", "parsed", "preflight_admitted", "runtime_evidence"]);
                state
            }).collect());
        }
    }
    compact_context(&summary, 0)
}

/// Runs on the support worker; all retained warnings/errors, independent of UI filters.
fn support_text(
    snapshot: DiagnosticSnapshot,
    context: serde_json::Value,
) -> Result<String, String> {
    use std::fmt::Write;
    let status = &snapshot.status;
    let mut text = format!(
        "KONTRA diagnostics — concise session summary\n{}\n\n{} retained / {} total session events. Session totals: Debug {}, Info {}, Warning {}, Error {}.\n{} evicted from live view; {} recorder drops; {} abbreviated events; {} write errors; {} retention errors.\nCounts below cover retained events only. Previous sessions and rotated disk history are NOT included. Full support export preserves available raw journal history and inventories.\nPaths redacted; bounded source excerpts included only when retained.\n\n",
        crate::build_info::SUMMARY,
        snapshot.events.len(),
        status.total_events,
        status.level_counts[0],
        status.level_counts[1],
        status.level_counts[2],
        status.level_counts[3],
        status.history_evicted,
        status.dropped_events,
        status.truncated_events,
        status.write_errors,
        status.retention_errors
    );
    let indices: Vec<_> = (0..snapshot.events.len()).rev()
        .filter(|&n| matches!(snapshot.events[n].level, LogLevel::Warning | LogLevel::Error)).collect();
    // Identity uses original paths; redaction must not merge distinct same-named items.
    let groups = group_events(&snapshot.events, &indices);
    let mut safe = json!({"context":context,"events":snapshot.events});
    diagnostics::clean(&mut safe, true);
    let events: Vec<LogEvent> = serde_json::from_value(safe["events"].take()).map_err(|error| error.to_string())?;
    let _ = writeln!(
        text,
        "WARNINGS AND ERRORS — {} groups / {} retained occurrences",
        groups.len(),
        indices.len()
    );
    if groups.is_empty() {
        text.push_str("No warnings or errors in the retained session view.\n");
    }
    for group in groups.iter().take(128) {
        text.push('\n');
        let detail = group_details(&events, group);
        if text.len() + detail.len() > 128 * 1024 {
            let _ = writeln!(
                text,
                "Remaining failure details omitted at 128 KiB; {} groups total. Full support export preserves records.",
                groups.len()
            );
            break;
        }
        text.push_str(&detail);
    }
    if groups.len() > 128 {
        let _ = writeln!(
            text,
            "{} more failure groups available in full support export.",
            groups.len() - 128
        );
    }
    text.push_str(
        "\nCURRENT CONFIGURATION (inventory and raw history available in full support export)\n",
    );
    text.push_str(
        &serde_json::to_string_pretty(&support_context(&safe["context"]))
            .map_err(|error| error.to_string())?,
    );
    Ok(text)
}


fn draw(ui: &mut Ui, state: &mut State, params: &Arc<SamplerParams>) -> El {
    if let Some(text) = state.copy_ready.take() {
        ui.set_clipboard(text);
        state.copy_message = Some("Copied concise summary. Full export includes raw history and inventories.".into());
    }
    let snapshot = state.snapshot.clone();
    if let Some(snapshot) = &snapshot {
        state.cache_receipt(snapshot);
    }
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
        "Export full support report…",
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
    let (copy, copy_el) = action(ui, "logs-copy-all", "Copy summary", false);
    if copy && state.copy_thread.is_none() {
        let params = params.clone();
        let answer = state.copy_answer.clone();
        state.copy_error = None;
        state.copy_message = Some("Preparing concise diagnostics…".into());
        match std::thread::Builder::new().name("kontra-copy-diagnostics".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let context = params.diagnostic_report();
                support_text(diagnostics::snapshot(), context)
            })).unwrap_or_else(|_| Err("Could not collect diagnostics. Retry Copy summary.".into()));
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
    if let Some(receipt) = &state.receipt {
        let mut summary = vec![
            caption(receipt.status.clone())
                .fill(secondary())
                .lines(2)
                .id("logs-report-status"),
        ];
        if let Some(url) = &receipt.issue_url {
            summary.push(
                caption(url.clone())
                    .fill(secondary())
                    .lines(2)
                    .id("logs-report-issue"),
            );
        }
        let mut receipt_row = vec![col(summary).gap(0).flex(1).min_w(0)];
        if let Some(url) = &receipt.issue_url {
            let (copy, button) = action(ui, "logs-copy-issue", "Copy issue link", false);
            if copy {
                ui.set_clipboard(url.clone());
            }
            receipt_row.push(button);
        }
        content.push(
            row(receipt_row)
                .gap(INSET)
                .align(Align::Center)
                .pad((INSET, TIGHT))
                .shrink(0)
                .id("logs-report-receipt"),
        );
    }
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
            "{} groups / {} matching events / {} retained (up to {}) · {} session events · {} write errors · UTC",
            state.matches.len(),
            state.matching_events,
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
        let group = &state.groups[i];
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
        let stage = event_stage(event);
        let code = event.code.as_deref().unwrap_or(&event.event);
        let patch = event.path.as_deref().map(filename)
            .or(event.library.as_deref()).unwrap_or("Application");
        let title = if inventory(event) {
            format!("{} · {patch} · inventory {}", level_name(event.level), inventory_field(event))
        } else if group.members.len() > 1 {
            format!(
                "{} ×{} · {} items/locations · {stage} / {code}",
                level_name(event.level),
                group.members.len(),
                group.children.len()
            )
        } else {
            format!("{} · {patch} · {stage} / {code}", level_name(event.level))
        };
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
                row![body(group_reason(&snapshot.events, group))
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
        let (copy, copy_el) = action(ui, "logs-copy", if state.raw_details { "Copy raw record" } else { "Copy group details" }, false);
        if state
            .detail
            .as_ref()
            .is_none_or(|(seq, _)| *seq != event.sequence)
        {
            let group = state.groups.iter().find(|group| snapshot.events[group.latest].sequence == event.sequence).unwrap();
            state.detail = Some((event.sequence, group_details(&snapshot.events, group).into()));
        }
        let (raw, raw_el) = action(ui, "logs-raw-details", "Latest raw record", state.raw_details);
        if raw { state.raw_details ^= true; }
        let full = if state.raw_details { Arc::<str>::from(details(event)) }
            else { state.detail.as_ref().unwrap().1.clone() };
        #[cfg(feature = "uvi")]
        let local_context=local_lua_context(params,event);

        if copy {
            ui.set_clipboard(full.to_string());
        }
        let (scope, scope_el) = action(ui, "logs-this-load", "This load", false);
        if scope && let Some(load) = &event.load_id {
            state.for_load(load);
        }
        content.push(rule());
        content.push(section_bar(
            if inventory(event) { "Inventory summary" } else { "Event details" },
            vec![
                raw_el,
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
        #[cfg(feature = "uvi")]
        if let Some(context)=local_context {
            content.push(col![body(context.display.clone()).text_size(TEXT).w(width.max(100.)).shrink(0)]
                .pad(INSET).h(180.).shrink(0).scroll().id("logs-local-lua-context"));
        } else if event.details["worker"]["lua_failure"]["local_source_excerpt_available"] == true {
            content.push(body("Local source context is no longer retained for this activation.")
                .text_size(TEXT).fill(secondary()).pad(INSET).shrink(0)
                .id("logs-local-lua-context-unavailable"));
        }
    } else {
        content.push(rule());
        content.push(section_bar("Event details", Vec::new()));
        content.push(
            col![
                caption("Select a group to inspect unique failing items and source context.")
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

    #[cfg(feature = "uvi")]
    #[test]
    fn local_lua_code_panel_is_activation_bound_and_excluded_from_copy_export() {
        let params=Arc::new(SamplerParams::new());
        let epoch=params.shared.uvi_activation_epoch();
        let source=crate::library::UviSource {bank:"/owned/Authored bank.ufs".into(),bank_uuid:[0;16],member:"Authored.uvip".into()};
        let context=Arc::new(crate::uvi::lua_failure::Context {processor:Some(2),frame:256,line:Some(3),
            chunk:"UVI ScriptProcessor node 2".into(),excerpt:Some(json!({"text":">      3 | private_owned_source_marker()"})),
            unavailable:"",display:"Local Lua source context (excluded from copy and export)\n>      3 | private_owned_source_marker()".into()});
        let published=Arc::new(crate::plugin::uvi_ui::Published {stamp:crate::uvi::worker::Stamp {epoch,generation:11,frame:0},
            snapshots:Arc::default(),pictures:Arc::default(),fonts:Arc::default()});
        let mut part=crate::plugin::PartView::authored_uvi(source.clone(),published);
        part.uvi_lua_failure=Some(context.clone());
        params.shared.view.lock().unwrap().parts[0]=part;
        let event:LogEvent=serde_json::from_value(json!({"schema_version":1,"sequence":1,"timestamp_ms":1000,"monotonic_ms":0,
            "session_id":"authored-local-code","level":"error","module":"uvi","event":"uvi_worker_failed","code":"uvi_worker_failed",
            "path":source.bank,"reason":"Authored failure","data":{"slot":0,"epoch":epoch,"generation":11,"member":source.member,
                "worker":{"lua_failure":context.metadata()}}})).unwrap();
        assert!(local_lua_context(&params,&event).is_some());
        for key in ["epoch","generation","member"] {
            let mut stale=event.clone();stale.details[key]=json!("changed");
            assert!(local_lua_context(&params,&stale).is_none());
        }
        let mut stale=event.clone();stale.path=Some("/owned/Other bank.ufs".into());
        assert!(local_lua_context(&params,&stale).is_none());
        for key in ["processor","frame","line","chunk"] {
            let mut stale=event.clone();stale.details["worker"]["lua_failure"][key]=json!("changed");
            assert!(local_lua_context(&params,&stale).is_none());
        }
        let snapshot=Arc::new(DiagnosticSnapshot {revision:1,events:vec![event],status:diagnostics::LogStatus::default(),build:json!({})});
        let board=Arc::new(Mutex::new(String::new()));
        let mut ui=super::super::theme::ui().clipboard(Board(board.clone()));
        let mut state=State::default();state.snapshot=Some(snapshot.clone());
        for _ in 0..3 {tick(&mut ui,&mut state,&params,Input::default());}
        press(&mut ui,&mut state,&params,"log-event-1");
        assert!(ui.scene().unwrap().surface("logs-local-lua-context").is_some());
        press(&mut ui,&mut state,&params,"logs-copy");
        assert!(!board.lock().unwrap().contains("private_owned_source_marker"));
        press(&mut ui,&mut state,&params,"logs-raw-details");
        press(&mut ui,&mut state,&params,"logs-copy");
        assert!(!board.lock().unwrap().contains("private_owned_source_marker"));
        assert!(!support_text((*snapshot).clone(),params.diagnostic_report()).unwrap().contains("private_owned_source_marker"));
        params.shared.view.lock().unwrap().parts[0].uvi_lua_failure=None;
        tick(&mut ui,&mut state,&params,Input::default());
        assert!(ui.scene().unwrap().surface("logs-local-lua-context").is_none());
        assert!(ui.scene().unwrap().surface("logs-local-lua-context-unavailable").is_some());
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
    fn repeated_failures_group_without_losing_causes_items_or_source() {
        let event = |n: u64, processor: &str, reason: &str| -> LogEvent {
            serde_json::from_value(json!({"schema_version":1,"sequence":n,"timestamp_ms":1000+n,
                "monotonic_ms":n,"session_id":"authored-group-test","level":"error","module":"uvi",
                "event":"uvi_audio_endpoint_failed","code":"uvi_audio_endpoint_failed","reason":reason,
                "path":"/private/Bank.ufs","data":{"processor":processor,"frame":n*256,"epoch":n,
                    "source_excerpt":{"text":">      8 | authored_failure()\n"},"access_key":"private-token"}})).unwrap()
        };
        let mut events: Vec<_> = (1..=743).map(|n| event(n,"Oscillator_A","sample unavailable")).collect();
        events.push(event(744,"Oscillator_B","sample unavailable"));
        events.push(event(745,"Oscillator_B","invalid processor argument"));
        let indices: Vec<_> = (0..events.len()).rev().collect();
        let groups = group_events(&events, &indices);
        assert_eq!(groups.len(), 1, "same error code has one parent; root causes stay distinct children");
        assert_eq!((groups[0].members.len(), groups[0].children.len()), (745, 3));
        assert_eq!((groups[0].first_ms, groups[0].last_ms), (1001, 1745));
        let mut state = State::default();
        let snapshot = DiagnosticSnapshot {revision:1,events:events.clone(),status:Default::default(),build:json!({})};
        state.selected = Some(10);
        assert!(state.filter(&snapshot));
        assert_eq!((state.matches.len(), state.matching_events), (1, 745));
        assert_eq!(state.selected, Some(745), "selection follows its retained group");
        state.search = "Oscillator_A".into();
        assert!(state.filter(&snapshot));
        assert_eq!((state.matches.len(), state.matching_events), (1, 743));
        assert!(group_details(&events, &state.groups[0]).contains("743 occurrences"));
        let report = support_text(snapshot, json!({"uvi":{"node_report_coverage":{"included_rows":9996},"program":{"nodes":vec![json!({"name":"inventory-noise"});9996]}},"parts":[{"load":{"issues_omitted":7}}]})).unwrap();
        assert!(report.len() < 5000, "743 repeated failures and 9996 inventory rows produce a small summary: {}", report.len());
        assert_eq!(report.matches("sample unavailable").count(), 2, "each different failing item keeps its exact root cause once");
        assert!(report.contains("745 occurrences") && report.contains("3 unique items"));
        assert!(report.contains("authored_failure()") && !report.contains("inventory-noise"));
        assert!(!report.contains("private-token") && !report.contains("/private/"));
        assert!(report.contains("9996") && report.contains("issues_omitted"));
        events[0].details = json!({"frame":1});
        let missing = group_details(&events, &group_events(&events, &[0])[0]);
        assert!(!missing.contains("Script source context unavailable"), "a UVI rack slot is not evidence of script source");
        let mut structured = events[0].clone();
        structured.details = json!({"error":{"message":"authored failure","frame":1,"timestamp_ms":100}});
        let mut later = structured.clone();
        later.sequence += 1;
        later.details["error"]["frame"] = json!(512);
        later.details["error"]["timestamp_ms"] = json!(200);
        assert_eq!(group_events(&[structured,later], &[1,0]).len(), 1, "volatile fields inside error context do not duplicate a cause");
        let mut other_session = events[0].clone();
        other_session.session_id = "second-session".into();
        events.push(other_session);
        assert_eq!(group_events(&events, &[0,events.len()-1]).len(), 2);
    }

    #[test]
    fn inventory_chunks_group_by_load_field_revision_with_truthful_retained_coverage() {
        let event = |seq: u64, load: &str, field: &str, revision: u64, chunk: u64, final_chunk: bool| -> LogEvent {
            serde_json::from_value(json!({"schema_version":1,"sequence":seq,"timestamp_ms":1000+seq,
                "monotonic_ms":seq,"session_id":"inventory-ui-fixture","level":"info","module":"loader",
                "event":"load_detail","code":"load_detail_inventory","stage":"uvi_graph_diagnosis_and_preflight",
                "load_id":load,"data":{"code":"load_detail_inventory","field":["native_program_graph",field],
                    "revision":revision,"chunk":chunk,"item_start":chunk*3,"items_total":9,"final":final_chunk,
                    "items":["authored-node-a","authored-node-b","authored-node-c"]}})).unwrap()
        };
        let mut events = vec![event(1,"load-A","nodes",7,0,false),event(2,"load-A","nodes",7,1,false),
            event(3,"load-A","nodes",7,1,false),event(4,"load-A","nodes",7,2,true),
            event(5,"load-B","nodes",7,2,true),event(6,"load-A","nodes",8,1,false),
            event(7,"load-A","connections",7,0,false)];
        for (seq,code) in [(8,"stage_started"),(9,"stage_finished")] {
            let mut transition = events[0].clone();
            transition.sequence = seq;
            transition.event = code.into();
            transition.code = Some(code.into());
            transition.details = json!({"message":"authored stage transition"});
            events.push(transition);
        }
        let snapshot = DiagnosticSnapshot {revision:1,events,status:diagnostics::LogStatus{total_events:9,level_counts:[0,9,0,0],..Default::default()},build:json!({})};
        let mut state = State::default();
        state.levels = [true;4];
        assert!(state.filter(&snapshot));
        assert_eq!((state.groups.len(),state.matching_events),(6,9));
        let group = state.groups.iter().find(|group|group.members.len()==4).unwrap();
        let text = group_details(&snapshot.events,group);
        assert!(text.contains("9 / 9 rows") && text.contains("3 / 3 chunks"),"duplicates count once: {text}");
        assert!(!text.contains("authored-node"),"inventory payload stays behind raw/export actions");
        let partial = state.groups.iter().find(|group|snapshot.events[group.latest].load_id.as_deref()==Some("load-B")).unwrap();
        let text = group_details(&snapshot.events,partial);
        assert!(text.contains("3 / 9 rows") && text.contains("1 / 3 chunks"),"a retained tail never claims full coverage: {text}");
        assert_eq!(state.groups.iter().filter(|group|snapshot.events[group.latest].event.starts_with("stage_")).count(),2);
        state.search = "authored-node-a".into();
        assert!(state.filter(&snapshot));
        assert_eq!((state.groups.len(),state.matching_events),(4,7),"search still inspects original chunk payloads before grouping");
        let incomplete = state.groups.iter().find(|group|snapshot.events[group.latest].details["revision"]==8).unwrap();
        assert!(inventory_summary(&snapshot.events,incomplete).contains("unknown total chunks"));
        let params = Arc::new(SamplerParams::new());
        let mut ui = super::super::theme::ui();
        state.snapshot = Some(Arc::new(snapshot));
        state.search.clear();
        for _ in 0..3 { tick(&mut ui, &mut state, &params, Input::default()); }
        assert_eq!(state.matches.len(),6);
        press(&mut ui,&mut state,&params,"log-event-4");
        assert!(state.detail.as_ref().unwrap().1.contains("9 / 9 rows"));
        assert!(!state.detail.as_ref().unwrap().1.contains("authored-node"));
        let path = Path::new("artifacts/diagnostics/log-inventory-fixture.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        moose::core::screenshot::save_png(path,&super::super::tests::pixels(&ui,900,700),900,700);
        press(&mut ui,&mut state,&params,"logs-raw-details");
        assert!(state.raw_details);
        assert!(details(&state.snapshot.as_ref().unwrap().events[3]).contains("authored-node-a"));
    }

    #[test]
    fn typed_endpoint_frames_deduplicate_but_nested_causes_and_locations_survive() {
        let event = |n: u64, frame: u64, processor: &str| -> LogEvent {
            serde_json::from_value(json!({"schema_version":1,"sequence":n,"timestamp_ms":1000+n,
                "monotonic_ms":n,"session_id":"typed-endpoint-fixture","level":"error","module":"uvi",
                "event":"uvi_audio_endpoint_failed","code":"uvi_audio_endpoint_failed",
                "reason":format!("RequestCapacity at request (frame {frame})"),
                "data":{"slot":0,"bank":"Bank.ufs","member":"Instrument.uvip","processor":processor,
                    "worker":{"failure":null,"status":"ready","stats":{"frame":frame}},
                    "endpoint":{"error":"RequestCapacity","error_code":"request_capacity","frame":frame,
                        "stage":"request","source":"src/uvi/endpoint.rs","line":81}}})).unwrap()
        };
        let mut first = event(1,256,"Processor_A");
        first.details["endpoint"]["source_excerpt"] = json!({"text":">     81 | authored_request_failure()\n"});
        let later = event(2,512,"Processor_A");
        let mut different = event(3,1024,"Processor_B");
        different.details["worker"]["failure"] = json!("authored worker processing error");
        let events = vec![first,later,different];
        let groups = group_events(&events,&[2,1,0]);
        assert_eq!((groups.len(),groups[0].members.len(),groups[0].children.len()),(1,3,2));
        assert_eq!(cause(&events[1]),"RequestCapacity");
        assert_eq!(cause(&events[2]),"authored worker processing error");
        let text = group_details(&events,&groups[0]);
        assert_eq!(text.matches("Cause: RequestCapacity").count(),1);
        assert!(text.contains("authored worker processing error"));
        assert!(text.contains("source: src/uvi/endpoint.rs") && text.contains("line: 81"));
        assert!(text.contains("authored_request_failure()"),"earlier real source excerpt survives later missing source");
        assert!(!text.contains("RequestCapacity at request (frame"),"volatile formatted reasons do not duplicate canonical causes");
        assert_eq!(group_reason(&events,&groups[0]),"2 unique item/cause/location entries · select to inspect");
        let mut unavailable = events[1].clone();
        unavailable.details["endpoint"]["source_kind"] = json!("rust");
        unavailable.details["endpoint"]["source_excerpt_unavailable"] = json!("Native source differs from the source used to build this binary");
        let unavailable_events = vec![unavailable];
        let unavailable_text = group_details(&unavailable_events,&group_events(&unavailable_events,&[0])[0]);
        assert!(unavailable_text.contains("Rust source context unavailable: Native source differs from the source used to build this binary"));
    }

    #[test]
    fn grouped_failure_rows_show_counts_and_real_source_before_raw_records() {
        let params = Arc::new(SamplerParams::new());
        let mut ui = super::super::theme::ui();
        let events = (1..=743).map(|n| serde_json::from_value(json!({
            "schema_version":1,"sequence":n,"timestamp_ms":1000+n,"monotonic_ms":n,
            "session_id":"authored-group-ui","level":"error","module":"uvi",
            "event":"uvi_audio_endpoint_failed","code":"uvi_audio_endpoint_failed",
            "reason":"Authored source failure","data":{"slot":0,"processor":"Oscillator_A",
                "frame":n,"source_excerpt":{"text":">      8 | authored_failure()\n"}}
        })).unwrap()).collect();
        let mut state = State::default();
        state.snapshot = Some(Arc::new(DiagnosticSnapshot {revision:1,events,
            status:diagnostics::LogStatus {total_events:743,level_counts:[0,0,0,743],..Default::default()},build:json!({})}));
        for _ in 0..3 { tick(&mut ui, &mut state, &params, Input::default()); }
        assert_eq!((state.matches.len(), state.matching_events), (1,743));
        assert!(ui.scene().unwrap().surface("log-event-743").is_some());
        assert!(ui.scene().unwrap().surface("log-event-742").is_none());
        press(&mut ui, &mut state, &params, "log-event-743");
        assert_eq!(state.selected, Some(743));
        let detail = &state.detail.as_ref().unwrap().1;
        assert!(detail.contains("743 occurrences") && detail.contains("authored_failure()"));
        assert!(!detail.contains("Complete event record"));
        assert!(ui.scene().unwrap().surface("logs-raw-details").is_some());
        let path = Path::new("artifacts/diagnostics/log-grouped-fixture.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        moose::core::screenshot::save_png(path, &super::super::tests::pixels(&ui,900,700),900,700);
        press(&mut ui, &mut state, &params, "logs-raw-details");
        assert!(state.raw_details, "raw record stays explicitly available");
    }

    #[test]
    fn crash_receipts_remain_visible_with_default_filters_and_copy_only_public_issues() {
        let params = Arc::new(SamplerParams::new());
        let clipboard = Arc::new(Mutex::new(String::new()));
        let mut ui = super::super::theme::ui().clipboard(Board(clipboard.clone()));
        let mut state = State::default();
        let snapshot = |sent: bool, manual: bool, url: &str, event: &str| {
            Arc::new(DiagnosticSnapshot {
                revision: 1,
                events: vec![
                    serde_json::from_value(json!({
                        "schema_version":1,"sequence":1,"timestamp_ms":1759392000000u64,
                        "monotonic_ms":0,"session_id":"synthetic-receipt-ui-fixture",
                        "level":"info","module":"support","event":event,
                        "data":{"sent":sent,"manual_export_required":manual,
                            "report_id":"report-fixture-42","issue_url":url}
                    }))
                    .unwrap(),
                ],
                status: Default::default(),
                build: json!({"fixture":true}),
            })
        };
        let url = "https://github.com/DerpcatMusic/KONTRA/issues/42";
        for event in ["automatic_crash_report", "previous_crash_report"] {
            state.snapshot = Some(snapshot(true, false, url, event));
            for _ in 0..3 {
                tick(&mut ui, &mut state, &params, Input::default());
            }
            assert_eq!(state.levels, [false, false, true, true]);
            assert!(
                state.matches.is_empty(),
                "Info receipt stays filtered out of the history"
            );
            let scene = ui.scene().unwrap();
            for id in ["logs-report-status", "logs-report-issue", "logs-copy-issue"] {
                assert!(
                    scene.surface(id).is_some_and(|s| s.frame.size.height > 0.),
                    "live/restored receipt is visible above the default filters: {id}"
                );
            }
            assert!(
                state
                    .receipt
                    .as_ref()
                    .unwrap()
                    .status
                    .contains("report-fixture-42")
            );
            press(&mut ui, &mut state, &params, "logs-copy-issue");
            assert_eq!(*super::super::lock(&clipboard), url);
            state.search = "no matching log event".into();
            tick(&mut ui, &mut state, &params, Input::default());
            assert!(ui.scene().unwrap().surface("logs-report-receipt").is_some());
            assert!(
                Arc::ptr_eq(
                    state.receipt_snapshot.as_ref().unwrap(),
                    state.snapshot.as_ref().unwrap()
                ),
                "unchanged snapshots reuse their receipt independently of search/filter changes"
            );
        }
        for rejected in [
            "https://github.com/DerpcatMusic/buffr-support/issues/42",
            "https://example.com/DerpcatMusic/KONTRA/issues/42",
            "https://github.com/DerpcatMusic/KONTRA/issues/42?token=private",
            "https://github.com/DerpcatMusic/KONTRA/issues/0",
        ] {
            // Same revision and shape, but a new immutable snapshot must replace
            // the old safe-link cache instead of leaving its button active.
            state.snapshot = Some(snapshot(true, false, rejected, "previous_crash_report"));
            for _ in 0..3 {
                tick(&mut ui, &mut state, &params, Input::default());
            }
            assert!(ui.scene().unwrap().surface("logs-report-status").is_some());
            assert!(ui.scene().unwrap().surface("logs-copy-issue").is_none());
            assert!(ui.scene().unwrap().surface("logs-report-issue").is_none());
        }
        for manual in [false, true] {
            state.snapshot = Some(snapshot(false, manual, url, "automatic_crash_report"));
            for _ in 0..3 {
                tick(&mut ui, &mut state, &params, Input::default());
            }
            assert!(
                ui.scene().unwrap().surface("logs-copy-issue").is_none(),
                "a failed receipt cannot offer even an otherwise valid issue URL"
            );
            let status = &state.receipt.as_ref().unwrap().status;
            assert!(status.contains(if manual {
                "manual export"
            } else {
                "retained for retry"
            }));
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
                code: Some(format!("resolved_reference_{n}")),
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
            revision: 1,
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
        let mut cached = State::default();
        assert!(cached.filter(&snapshot));
        let mut replacement = snapshot.clone();
        replacement.status.log_path = Some("/virtual/logs/replacement-session.jsonl".into());
        replacement.events[0].level = LogLevel::Warning;
        assert_eq!(replacement.revision, snapshot.revision);
        assert_eq!(replacement.events.len(), snapshot.events.len());
        assert!(cached.filter(&replacement), "same revision and row count from another journal invalidates the cache");
        assert!(cached.matches.contains(&0), "replacement history must supply the rebuilt matching rows");
        replacement.events.truncate(1);
        assert!(cached.filter(&replacement), "same revision and journal with a replaced history shape invalidates the cache");
        assert_eq!(cached.matches, vec![0], "stale retained indices cannot outlive the replacement rows");
        assert!(!cached.filter(&replacement), "unchanged replacement history still reuses the cache");
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
        assert!(!detail.contains("Complete event record"), "group details keep the raw record behind its own action");
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
        assert!(
            copied.contains("Marker 2047:") && !copied.contains("Marker 0:"),
            "summary includes retained failure groups, independent of search, with progress in full export"
        );
        assert!(
            copied.contains("7952")
                && copied.contains("10000")
                && copied.contains("issues_omitted")
                && copied.contains("omitted locations unknown"),
            "recorder coverage and load/runtime omissions remain distinct"
        );
        assert!(
            copied.contains("Diagnostic Test.nki")
                && !copied.contains("/virtual/private-user")
                && !copied.contains("private script payload"),
            "default redaction keeps filenames and removes private payloads"
        );
        assert!(copied.contains("Previous sessions and rotated disk history are NOT included"));
        assert!(
            copied.find("WARNINGS AND ERRORS").unwrap()
                < copied.find("CURRENT CONFIGURATION").unwrap()
        );
        assert!(copied.contains("more failure groups available in full support export"));
        assert!(!copied.contains("private-token") && !copied.contains(r"C:\virtual"));
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
            assert!(std::time::Instant::now() < until, "Copy summary worker did not finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(state.copy_error.is_none(), "{:?}", state.copy_error);
        let copied = super::super::lock(&clipboard).clone();
        assert!(copied.starts_with("KONTRA diagnostics") && copied.contains(crate::build_info::SUMMARY) && copied.contains("CURRENT CONFIGURATION"), "the actual Copy summary action publishes worker report text to the clipboard");

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
