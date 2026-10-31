//! The session log: one JSON line per event, written and flushed as it
//! happens to `~/.cache/kontra/logs/session-<pid>-<stamp>-1.jsonl`. A host
//! shows what the editor draws and nothing else, so a failure the editor
//! cannot draw (a library that did not open, a page that got no pictures)
//! is written here instead of vanishing.
//!
//! `KONTRA_LOG_DIR` puts the files elsewhere and `KONTRA_LOG=0` turns the
//! log off; anything else is quiet: errors only. Files rotate at
//! [`MAX_BYTES`] and the newest [`KEEP`] sessions are kept, so a log can
//! never fill the disk. [`start`] writes the first line, with the path, so
//! the file is findable from the plugin alone.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use kontra_host::ui::Missing;

/// Bytes one session file may reach before it stops taking writes.
const MAX_BYTES: u64 = 4 << 20;
/// Session files kept; the oldest beyond this are removed at start.
const KEEP: usize = 16;

struct Log {
    file: Option<File>,
    path: PathBuf,
    bytes: u64,
    full: bool,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

/// Where the session log is, and disabled or not, from the environment.
fn place() -> (PathBuf, bool) {
    if let Some(dir) = std::env::var_os("KONTRA_LOG_DIR").filter(|v| !v.is_empty()) {
        return (PathBuf::from(dir), true);
    }
    let quiet = std::env::var_os("KONTRA_LOG").is_some_and(|v| v == "0" || v == "off");
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("kontra")
        .join("logs");
    (dir, !quiet)
}

/// The session log's file, opened on first use.
fn log() -> &'static Mutex<Log> {
    LOG.get_or_init(|| {
        let (dir, on) = place();
        let _ = std::fs::create_dir_all(&dir);
        prune(&dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = dir.join(format!("session-{}-{stamp}-1.jsonl", std::process::id()));
        let file = on
            .then(|| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .ok()
            })
            .flatten();
        Mutex::new(Log {
            file,
            path,
            bytes: 0,
            full: false,
        })
    })
}

/// Keeps the newest [`KEEP`] session files and removes the rest.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = (entries)
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("session-"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (_, path) in files.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(path);
    }
}

/// The session log's path, for a report line; empty when it is not open.
#[must_use]
pub fn path() -> PathBuf {
    log().lock().map(|l| l.path.clone()).unwrap_or_default()
}

/// Writes the session's first line, with the path: the log exists from here
/// on, whether or not anything goes wrong. Called on every tick; the first
/// call wins.
pub fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let path = path();
        write_line(
            "info",
            "session_started",
            &[
                ("log", &path.display().to_string()),
                ("build", crate::BUILD),
                ("pid", &std::process::id().to_string()),
            ],
        );
    });
}

/// An error: what happened, and the values that say where.
pub fn error(event: &str, data: &[(&str, &str)]) {
    write_line("error", event, data);
}

/// A significant state change, at most once a second per event name, so a
/// drag or a load cannot flood the log. The control side writes these,
/// never the audio thread.
pub fn info(event: &str, data: &[(&str, &str)]) {
    static LAST: [AtomicU64; 4] = [
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ];
    // A slot per event name: two different events never silence each other.
    let slot = (event.as_bytes().first().copied().unwrap_or(0) as usize) % LAST.len();
    let now = now_ms();
    let last = LAST.get(slot).map_or(0, |at| at.load(Ordering::Relaxed));
    if now.saturating_sub(last) < 1000 {
        return;
    }
    if let Some(at) = LAST.get(slot) {
        at.store(now, Ordering::Relaxed);
    }
    write_line("info", event, data);
}

/// Milliseconds since the plugin started, for [`info`]'s rate.
fn now_ms() -> u64 {
    static AT: OnceLock<std::time::Instant> = OnceLock::new();
    AT.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// One JSON line, written and flushed now; dropped once the file is full.
fn write_line(level: &str, event: &str, data: &[(&str, &str)]) {
    let Ok(mut log) = log().lock() else {
        return;
    };
    write(&mut log, level, event, data);
}

/// [`write_line`], for a log the caller holds.
fn write(log: &mut Log, level: &str, event: &str, data: &[(&str, &str)]) {
    if log.full {
        return;
    }
    let mut text = String::with_capacity(128);
    text.push_str("{\"level\":\"");
    text.push_str(level);
    text.push_str("\",\"event\":\"");
    push_escaped(&mut text, event);
    text.push('"');
    for (key, value) in data {
        text.push_str(",\"");
        push_escaped(&mut text, key);
        text.push_str("\":\"");
        push_escaped(&mut text, value);
        text.push('"');
    }
    text.push_str("}\n");
    let size = text.len() as u64;
    let Some(file) = log.file.as_mut() else {
        return;
    };
    if log.bytes + size > MAX_BYTES {
        log.full = true;
        let _ = file.write_all(b"{\"level\":\"error\",\"event\":\"log_full\"}\n");
        let _ = file.flush();
        return;
    }
    if file.write_all(text.as_bytes()).is_ok() {
        let _ = file.flush();
        log.bytes += size;
    }
}

/// `text` with JSON's escapes; control characters become spaces (a path or a
/// reason never needs them).
fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
}

/// A part's picture failures, for the loader of the face of `instrument`:
/// the first one with its reason and asset, then one line per 64 with the
/// running count. A frame asks for hundreds of pictures, and one line each
/// would bury the reason; a page that stays pictureless still says so, and
/// how many it lost.
pub fn picture_failures(instrument: Option<PathBuf>) -> Box<dyn Fn(Missing, &str) + Send + Sync> {
    /// Failures between two counted lines.
    const PICTURES_PER_LINE: usize = 64;
    let seen = AtomicUsize::new(0);
    let instrument = instrument.map_or_else(String::new, |p| p.display().to_string());
    Box::new(move |missing: Missing, asset: &str| {
        let n = seen.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 || n % PICTURES_PER_LINE == 0 {
            error(
                "picture_missing",
                &[
                    ("reason", &missing.to_string()),
                    ("asset", asset),
                    ("instrument", &instrument),
                    ("count", &n.to_string()),
                ],
            );
        }
    })
}

/// A picture too big for the renderer's atlas, said once: past that size
/// vello draws nothing and the page loses it without a word.
pub fn picture_too_large(side: u32, limit: u32) {
    static SAID: AtomicUsize = AtomicUsize::new(0);
    if SAID.swap(1, Ordering::Relaxed) == 0 {
        error(
            "picture_too_large",
            &[("side", &side.to_string()), ("limit", &limit.to_string())],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_one_json_object_with_its_escapes() {
        let mut text = String::new();
        push_escaped(&mut text, "a\"b\\c\nd");
        assert_eq!(text, "a\\\"b\\\\c\\nd");
    }

    #[test]
    fn a_line_reaches_the_file_as_it_is_written() {
        let dir = std::env::temp_dir().join(format!("kontra-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let file = dir.join("session-1-1-1.jsonl");
        let mut log = Log {
            file: OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file)
                .ok(),
            path: file.clone(),
            bytes: 0,
            full: false,
        };
        write(
            &mut log,
            "error",
            "picture_missing",
            &[("reason", "no library"), ("asset", "pictures/x.png")],
        );
        // Read while the writer is still alive: the line is on disk already.
        let text = std::fs::read_to_string(&file).expect("the line is flushed");
        assert_eq!(
            text,
            "{\"level\":\"error\",\"event\":\"picture_missing\",\"reason\":\"no library\",\"asset\":\"pictures/x.png\"}\n"
        );
    }
}
