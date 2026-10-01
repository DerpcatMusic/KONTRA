//! Global loader/runtime journal. Snapshot copies and report export stay off the UI thread.
use super::{Cx, menu, theme::*};
use crate::diagnostics::{self, DiagnosticSnapshot, ExportStatus, LogEvent, LogLevel};
use crate::plugin::SamplerParams;
use moose::mui::mui::prelude::*;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

const ROW: f64 = TEXT * 3. + SPACE;
static WAKE: AtomicU64 = AtomicU64::new(0);

/// A completed snapshot also wakes an idle plugin editor.
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
    snapshot: Option<Arc<DiagnosticSnapshot>>,
    requested: Option<u64>,
    read_error: Option<String>,
    search: String,
    library: String,
    patch: String,
    pub(super) load: String,
    levels: [bool; 4],
    filtered: Option<(u64, String, String, String, String, [bool; 4])>,
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
}
impl Default for State {
    fn default() -> Self {
        Self {
            reader: Arc::default(),
            reader_thread: None,
            export_thread: None,
            export_answer: Arc::default(),
            snapshot: None,
            requested: None,
            read_error: None,
            search: String::new(),
            library: String::new(),
            patch: String::new(),
            load: String::new(),
            levels: [false, true, true, true],
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
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        // The plugin library cannot unload while one of its workers is running.
        for handle in [self.reader_thread.take(), self.export_thread.take()]
            .into_iter()
            .flatten()
        {
            let _ = handle.join();
        }
    }
}
impl State {
    fn refresh(&mut self) {
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
        let spawned = std::thread::Builder::new()
            .name("kontra-log-view".into())
            .spawn(move || {
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
            self.library.clone(),
            self.patch.clone(),
            self.load.clone(),
            self.levels,
        );
        if self.filtered.as_ref() == Some(&key) {
            return false;
        }
        let needle = self.search.to_lowercase();
        let words: Vec<_> = needle.split_whitespace().collect();
        let library = self.library.trim().to_lowercase();
        let patch = self.patch.trim().to_lowercase();
        let load = self.load.trim();
        let criteria_changed = self.filtered.as_ref().is_none_or(|old| {
            (&old.1, &old.2, &old.3, &old.4, old.5) != (&key.1, &key.2, &key.3, &key.4, key.5)
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
                if !library.is_empty()
                    && !event
                        .library
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&library)
                {
                    return None;
                }
                if !patch.is_empty()
                    && !event
                        .path
                        .as_deref()
                        .unwrap_or_default()
                        .to_lowercase()
                        .contains(&patch)
                {
                    return None;
                }
                if !load.is_empty() && event.load_id.as_deref() != Some(load) {
                    return None;
                }
                if !words.is_empty() {
                    let hay = format!(
                        "{} {} {} {} {} {} {} {} {}",
                        event.module,
                        event.event,
                        event.stage.as_deref().unwrap_or_default(),
                        event.code.as_deref().unwrap_or_default(),
                        event.reason.as_deref().unwrap_or_default(),
                        event.library.as_deref().unwrap_or_default(),
                        event.path.as_deref().unwrap_or_default(),
                        event.load_id.as_deref().unwrap_or_default(),
                        event.details
                    )
                    .to_lowercase();
                    if !words.iter().all(|w| hay.contains(w)) {
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
    cx.state.logs.refresh();
    draw(ui, &mut cx.state.logs, cx.p)
}

fn field(ui: &mut Ui, id: &str, text: &mut String, label: &str) -> El {
    col![
        caption(label).fill(secondary()),
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
fn details(event: &LogEvent) -> String {
    serde_json::to_string_pretty(event).unwrap_or_else(|_| "Could not format this event.".into())
}

fn draw(ui: &mut Ui, state: &mut State, params: &Arc<SamplerParams>) -> El {
    let snapshot = state.snapshot.clone();
    let status = snapshot.as_ref().map(|s| &s.status);
    let (refresh, refresh_el) = action(ui, "logs-refresh", "Refresh", false);
    if refresh {
        state.requested = None;
        state.refresh();
    }
    let (open, open_el) = action(ui, "logs-folder", "Open log folder", false);
    let path = status
        .and_then(|s| s.log_path.clone())
        .or_else(diagnostics::log_path);
    if open {
        state.folder_error = match path.as_deref() {
            Some(path) => menu::reveal(path).err(),
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
    let mut content = vec![section_bar("Logs", vec![refresh_el, open_el, export_el])];
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
                .lines(5)
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
    for error in [&state.read_error, &state.folder_error, &state.export_error]
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
            "Redact local paths in the report",
            state.redact,
        );
        if redact {
            state.redact ^= true;
        }
        content.push(col![
            body("Support report preview").text_size(TEXT),
            caption(crate::build_info::LABEL).fill(secondary()).tip(crate::build_info::SUMMARY).lines(2),
            caption("Build and audio settings, load summaries, recent events and rotated logs. No samples, scripts or access keys.").fill(secondary()).lines(3),
            field(ui, "logs-export-path", &mut state.destination, "New report folder"),
            row![redact_el, caption(if state.redact { "Local paths are redacted" } else { "Local paths will be included" }).lines(2)].gap(SPACE).align(Align::Center),
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
            false,
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
        let text = match export {
            ExportStatus::Running => "Exporting the report in the background…".to_owned(),
            ExportStatus::Complete { path } => format!("Report saved to {}", path.display()),
            ExportStatus::Failed { error } => {
                format!("Export failed: {error}. Choose a new writable folder and retry.")
            }
        };
        content.push(
            body(text)
                .text_size(TEXT)
                .lines(3)
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
            "Search messages, stages and reasons"
        )]
        .pad((INSET, TIGHT))
        .shrink(0),
    );
    content.push(
        row![
            field(ui, "logs-library", &mut state.library, "Library"),
            field(ui, "logs-patch", &mut state.patch, "Patch or path"),
            field(ui, "logs-load", &mut state.load, "Exact load ID")
        ]
        .gap(SPACE)
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
        state.library.clear();
        state.patch.clear();
        state.load.clear();
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
        caption(format!(
            "{} matching / {} retained (up to {}) · {} session events · {} write errors · UTC",
            state.matches.len(),
            snapshot.events.len(),
            diagnostics::HISTORY_LIMIT,
            snapshot.status.total_events,
            snapshot.status.write_errors
        ))
        .fill(secondary())
        .lines(2)
        .pad((INSET, TIGHT))
        .shrink(0)
        .id("logs-count"),
    );
    if snapshot.status.history_evicted > 0
        || snapshot.status.dropped_events > 0
        || snapshot.status.truncated_events > 0
        || snapshot.status.retention_errors > 0
    {
        content.push(caption(format!("{} older events left the live view · {} dropped · {} oversized events abbreviated · {} retention errors. Export includes available rotated history.", snapshot.status.history_evicted, snapshot.status.dropped_events, snapshot.status.truncated_events, snapshot.status.retention_errors))
            .fill(secondary()).lines(2).pad((INSET, TIGHT)).shrink(0));
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
        if ui.get(id.as_str()).activated() {
            state.selected = Some(event.sequence);
        }
        let selected = state.selected == Some(event.sequence);
        let stage = event.stage.as_deref().unwrap_or(&event.module);
        let code = event.code.as_deref().unwrap_or(&event.event);
        let title = format!(
            "{} · {} · {stage} / {code}",
            time(event.timestamp_ms),
            level_name(event.level)
        );
        let scope = format!(
            "{} · {} · load {}{}",
            event.library.as_deref().unwrap_or("Application"),
            event
                .path
                .as_deref()
                .and_then(|p| Path::new(p).file_name())
                .unwrap_or_default()
                .to_string_lossy(),
            event.load_id.as_deref().unwrap_or("—"),
            event
                .line
                .map_or(String::new(), |line| format!(" · line {line}"))
        );
        items.push(interactive(
            col![
                caption(title)
                    .fill(if event.level == LogLevel::Error {
                        Fill::from(Role::Warning)
                    } else {
                        secondary()
                    })
                    .lines(1)
                    .min_w(0),
                body(event.reason.as_deref().unwrap_or(&event.event))
                    .text_size(TEXT)
                    .lines(1)
                    .min_w(0),
                caption(scope).fill(secondary()).lines(1).min_w(0),
            ]
            .gap(0)
            .align(Align::Stretch)
            .h(ROW)
            .pad((INSET, 1.))
            .shrink(0)
            .when(selected, |e| e.fill(Role::Raised))
            .focusable()
            .a11y(A11y::Button)
            .named(format!(
                "{}: {}",
                level_name(event.level),
                event.reason.as_deref().unwrap_or(&event.event)
            ))
            .tip(event.reason.as_deref().unwrap_or(&event.event).to_owned())
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
        let full = &state.detail.as_ref().unwrap().1;
        if copy {
            ui.set_clipboard(full.to_string());
        }
        let (scope, scope_el) = action(ui, "logs-this-load", "This load", false);
        if scope && let Some(load) = &event.load_id {
            state.load = load.clone();
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
                body(full.clone())
                    .text_size(TEXT)
                    .w(width.max(100.))
                    .shrink(0)
            ]
            .pad(INSET)
            .h(120.)
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

    #[test]
    fn the_global_log_panel_filters_and_virtualizes_retained_history() {
        let params = Arc::new(SamplerParams::new());
        let mut ui = super::super::theme::ui();
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
                path: Some(format!(
                    "/virtual/{}/Instruments/{}.nki",
                    if n % 2 == 0 {
                        "Fixture Keys"
                    } else {
                        "Fixture Strings"
                    },
                    if n % 2 == 0 { "Cello" } else { "Violin" }
                )),
                library: Some(if n % 2 == 0 {
                    "Fixture Keys".into()
                } else {
                    "Fixture Strings".into()
                }),
                program: Some(0),
                part: Some(0),
                script_slot: Some(0),
                line: Some(42),
                reason: Some(format!("Marker {n}: sample reference resolved.")),
                details: json!({"reason":"synthetic test event","line":42}),
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
        let mut state = State {
            snapshot: Some(Arc::new(snapshot)),
            ..Default::default()
        };
        for _ in 0..3 {
            tick(&mut ui, &mut state, &params, Input::default());
        }
        assert_eq!(
            state.matches.len(),
            1536,
            "debug is hidden, other levels remain searchable"
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
            Some(2),
            "End reaches the oldest matching event"
        );
        assert!(ui.scene().unwrap().surface("log-event-2").is_some());
        type_into(&mut ui, &mut state, &params, "logs-search", "Marker 1001");
        type_into(
            &mut ui,
            &mut state,
            &params,
            "logs-library",
            "Fixture Strings",
        );
        type_into(&mut ui, &mut state, &params, "logs-patch", "Violin");
        type_into(&mut ui, &mut state, &params, "logs-load", "15");
        assert_eq!(
            state.matches.len(),
            1,
            "search and all three scopes intersect"
        );
        press(&mut ui, &mut state, &params, "log-event-1002");
        assert_eq!(state.selected, Some(1002));
        assert!(ui.scene().unwrap().surface("logs-copy").is_some());
        assert!(state.detail.as_ref().unwrap().1.contains("Marker 1001"));
        let shot = Path::new("artifacts/diagnostics/log-panel-fixture.png");
        std::fs::create_dir_all(shot.parent().unwrap()).unwrap();
        moose::core::screenshot::save_png(
            shot,
            &super::super::tests::pixels(&ui, 900, 700),
            900,
            700,
        );
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
    }
}
