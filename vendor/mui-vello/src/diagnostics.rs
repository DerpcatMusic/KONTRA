//! Persistent MUI breadcrumbs and errors. No process-wide panic or signal hook.
//!
//! Native hosts record startup stages before calling a driver. Files rotate at
//! 1 MiB, retaining one previous file. [`log_path`] locates the current session.
//! With the `reporting` feature, `Reporter` also queues errors for MUI support.
//! This is a UI-thread API; do not call it from an audio callback.

use std::{any::Any, path::PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::{self, Write},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
mod reporting;
#[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
pub use reporting::{Config, Reporter};

/// Retains reporting through native resource teardown; empty without reporting.
pub struct ReportingGuard {
    #[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
    _reporter: Option<Reporter>,
}

/// Window hosts keep this as their last field, after native resources.
pub fn retain_reporter() -> ReportingGuard {
    ReportingGuard {
        #[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
        _reporter: reporting::active_reporter(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
const LOG_BYTES: u64 = 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MESSAGE_BYTES: usize = 2048;
#[cfg(not(target_arch = "wasm32"))]
const HISTORY: usize = 32;

#[cfg(not(target_arch = "wasm32"))]
struct Journal {
    path: PathBuf,
    history: VecDeque<String>,
}

#[cfg(not(target_arch = "wasm32"))]
static JOURNAL: OnceLock<Mutex<Journal>> = OnceLock::new();

/// Native log directory. `MUI_DIAGNOSTICS_DIR` overrides the platform default.
#[cfg(not(target_arch = "wasm32"))]
pub fn directory() -> PathBuf {
    if let Some(path) = std::env::var_os("MUI_DIAGNOSTICS_DIR").filter(|v| !v.is_empty()) {
        return path.into();
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("mui-diagnostics-tests-{}", std::process::id()));
    }
    let root = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|v| PathBuf::from(v).join("Library/Logs"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|v| PathBuf::from(v).join(".local/state")))
    };
    root.unwrap_or_else(std::env::temp_dir).join("MUI")
}

/// Persistent native diagnostics are unavailable in a browser.
#[cfg(target_arch = "wasm32")]
pub fn directory() -> PathBuf {
    PathBuf::new()
}

#[cfg(not(target_arch = "wasm32"))]
fn journal() -> &'static Mutex<Journal> {
    JOURNAL.get_or_init(|| {
        let directory = directory();
        // ponytail: retain the newest 64 session files; pending reports have
        // their own retention and are never pruned by local log housekeeping.
        if let Ok(entries) = std::fs::read_dir(&directory) {
            let mut files = entries
                .filter_map(Result::ok)
                .filter(|e| {
                    e.file_name().to_string_lossy().starts_with("session-")
                        && e.path().extension().is_some_and(|e| e == "log")
                })
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect::<Vec<_>>();
            files.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
            for (_, path) in files.into_iter().skip(64) {
                let _ = std::fs::remove_file(path);
            }
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Mutex::new(Journal {
            path: directory.join(format!("session-{}-{stamp:x}.log", std::process::id())),
            history: VecDeque::new(),
        })
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn bounded(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
pub(super) fn private_file(path: &std::path::Path) -> io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(not(target_arch = "wasm32"))]
impl Journal {
    fn append(&mut self, line: &str) -> io::Result<()> {
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(line.to_owned());
        std::fs::create_dir_all(
            self.path
                .parent()
                .ok_or_else(|| io::Error::other("no log directory"))?,
        )?;
        if self.path.metadata().is_ok_and(|m| m.len() >= LOG_BYTES) {
            let previous = self.path.with_extension("previous.log");
            if previous.exists() {
                std::fs::remove_file(&previous)?;
            }
            std::fs::rename(&self.path, previous)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.path)?;
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        writeln!(file, "{time} {line}")?;
        // No buffered user-space writer: keep the last stage on process abort.
        file.flush()
    }
}

/// The file for this loaded copy of MUI. Separate plugin images have separate sessions.
#[cfg(not(target_arch = "wasm32"))]
pub fn log_path() -> PathBuf {
    journal()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .path
        .clone()
}

/// Persistent native diagnostics are unavailable in a browser.
#[cfg(target_arch = "wasm32")]
pub fn log_path() -> PathBuf {
    PathBuf::new()
}

/// Save a stage before a native call. Breadcrumbs alone never create issues.
pub fn breadcrumb(component: &str, stage: &str, message: &str) {
    record(component, stage, message, false, None);
}

/// Persist a MUI error and, when a `Reporter` is retained, enqueue it for support.
/// Pass MUI-owned errors, not arbitrary host crashes or user content.
/// Use component `mui`, `mui-vello`, `mui-baseview` or `mui-winit` and a short
/// snake_case stage matching the support protocol.
pub fn error(component: &str, stage: &str, message: &str) {
    record(component, stage, message, true, None);
}

/// Include the device that actually raised the error, even when other windows
/// are using different adapters.
pub fn gpu_error(stage: &str, message: &str, adapter: &wgpu::AdapterInfo) {
    record("mui-vello", stage, message, true, Some(adapter));
}

/// Retain across a risky native GPU operation. Normal return and Rust unwind
/// clear it; abrupt process termination leaves an unconfirmed interruption
/// report for the next load. This installs no process-wide panic/signal hooks.
/// Use for startup/resize, not each animation frame or the audio thread.
#[must_use]
pub struct Operation {
    #[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
    _pending: Option<reporting::Operation>,
}

pub fn operation(
    component: &str,
    stage: &str,
    message: &str,
    adapter: Option<&wgpu::AdapterInfo>,
) -> Operation {
    breadcrumb(component, stage, message);
    #[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
    let pending = reporting::operation(component, stage, message, adapter);
    #[cfg(not(all(feature = "reporting", not(target_arch = "wasm32"))))]
    let _ = adapter;
    Operation {
        #[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
        _pending: pending,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn record(
    component: &str,
    stage: &str,
    message: &str,
    failed: bool,
    adapter: Option<&wgpu::AdapterInfo>,
) {
    let component = bounded(component, 32);
    let stage = bounded(stage, 48);
    let message = bounded(message, MESSAGE_BYTES);
    let history = {
        let mut log = journal()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let line = format!(
            "{} {component}/{stage}: {message}",
            if failed { "ERROR" } else { "INFO" }
        );
        if let Err(error) = log.append(&line) {
            eprintln!("MUI diagnostics could not write its journal: {error}");
        }
        if failed {
            log.history.iter().cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };
    #[cfg(all(feature = "reporting", not(target_arch = "wasm32")))]
    if failed {
        reporting::enqueue(component, stage, message, &history, adapter);
    }
    #[cfg(not(all(feature = "reporting", not(target_arch = "wasm32"))))]
    let _ = (history, adapter);
}

#[cfg(target_arch = "wasm32")]
fn record(_: &str, _: &str, _: &str, _: bool, _: Option<&wgpu::AdapterInfo>) {}

/// Preserve the payload of a caught Rust panic without installing a host-wide hook.
pub fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("panic without a string payload")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn journal_keeps_bounded_history_and_last_stage_across_rotation() {
        let path =
            std::env::temp_dir().join(format!("mui-journal-test-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("previous.log"));
        let mut log = Journal {
            path: path.clone(),
            history: VecDeque::new(),
        };
        for i in 0..(HISTORY + 1) {
            log.append(&format!("stage {i}")).unwrap();
        }
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(LOG_BYTES).unwrap();
        drop(file);
        log.append("before driver call").unwrap();
        assert_eq!(log.history.len(), HISTORY);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("before driver call")
        );
        assert_eq!(
            path.with_extension("previous.log")
                .metadata()
                .unwrap()
                .len(),
            LOG_BYTES
        );
        assert_eq!(bounded("éé", 3), "é");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(path.with_extension("previous.log")).unwrap();
    }
}
