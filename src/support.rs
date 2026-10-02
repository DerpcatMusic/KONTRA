//! BUFFR's crash recovery and acknowledged support transport, adapted for KONTRA.
//! No process-wide panic, signal or exception handler is installed in a host.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
mod crash;
mod platform;
mod report;
pub(crate) use crash::flush_journal;
pub use crash::{CrashIncident, CrashSessionGuard, pending_incident};
use report::try_auto_report_pending_incident;
const REPORT_URL: &str = "https://matari-audio.com/api/support/report";
static AUTOMATIC_CRASH_REPORT_STARTED: AtomicBool = AtomicBool::new(false);
static HOST_IDENTITY: LazyLock<Mutex<(String, String)>> =
    LazyLock::new(|| Mutex::new(Default::default()));

trait MutexExt<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T>;
}
impl<T> MutexExt<T> for Mutex<T> {
    fn lock_unpoisoned(&self) -> MutexGuard<'_, T> {
        self.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn support_cache_path() -> PathBuf {
    let root = std::env::var_os("KONTRA_REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(test) {
                return std::env::temp_dir()
                    .join(format!("kontra-support-tests-{}", std::process::id()));
            }
            dirs::data_local_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("KONTRA")
        });
    root.join("support-cache.json")
}
fn record_host_identity(format: &str, host: Option<&str>) {
    let mut identity = HOST_IDENTITY.lock_unpoisoned();
    identity.1 = format.to_owned();
    if let Some(host) = host {
        identity.0 = host.to_owned();
    }
}

pub fn register_crash_session() -> CrashSessionGuard {
    let (session, ready) = CrashSessionGuard::register();
    if ready {
        try_auto_report_pending_incident();
    }
    session
}

/// Off audio: each plugin's retained state owns one marker, before it constructs its rack.
pub(crate) fn start_plugin_session() -> Option<CrashSessionGuard> {
    // Tests use the explicit reporter entry points with isolated directories; plugin harnesses do not.
    if cfg!(test) {
        return None;
    }
    let host = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()));
    record_host_identity(plugin_format(), host.as_deref());
    let mut guard = register_crash_session();
    guard.mark_initializing(plugin_format(), host.as_deref());
    if !crash::capture_ready() {
        crate::diagnostics::event(
            crate::diagnostics::LogLevel::Warning,
            "support",
            "crash_capture_unavailable",
            serde_json::json!({"message":"Crash-session marker was not ready at plugin construction; check host stderr for storage/worker errors"}),
        );
    }
    Some(guard)
}

fn plugin_format() -> &'static str {
    #[cfg(unix)]
    {
        // SAFETY: the loader owns this path while our function is executing.
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        if unsafe { libc::dladdr(plugin_format as *const () as *const _, &mut info) } != 0
            && !info.dli_fname.is_null()
        {
            let path = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }
                .to_string_lossy()
                .to_lowercase();
            if path.contains(".clap") {
                return "CLAP";
            }
            if path.contains(".vst3") {
                return "VST3";
            }
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW,
        };
        let mut module = std::ptr::null_mut();
        // SAFETY: FROM_ADDRESS interprets this pointer as an address in our loaded image.
        if unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                plugin_format as *const () as *const _,
                &mut module,
            )
        } != 0
        {
            let mut path = [0u16; 32768];
            let length = unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) }
                as usize;
            let path = String::from_utf16_lossy(&path[..length.min(path.len())]).to_lowercase();
            if path.contains(".clap") {
                return "CLAP";
            }
            if path.contains(".vst3") {
                return "VST3";
            }
        }
    }
    if cfg!(feature = "standalone") {
        "standalone"
    } else {
        "unknown"
    }
}

/// Mirror every structured event, including large events, into the reused crash journal.
/// Called only by the existing diagnostics worker, never from process()/reset().
pub(crate) fn journal_event(event: &crate::diagnostics::LogEvent) {
    let Some(sink) = crash::diagnostic_sink() else {
        return;
    };
    mirror_event(&sink, event);
}
fn mirror_event(sink: &derpcat_flight_recorder::Sink, event: &crate::diagnostics::LogEvent) {
    use derpcat_flight_recorder::{Context, Phase, Subsystem};
    let mut value = serde_json::to_value(event).unwrap_or_default();
    sanitize_automatic(&mut value);
    let text = report::redact_log(&value.to_string());
    // BUFFR's recorder bounds one detail to 384 bytes; numbered UTF-8 chunks retain the complete event.
    let mut remaining = text.as_str();
    let mut index = 0;
    while !remaining.is_empty() {
        let mut end = remaining.len().min(320);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        let detail = format!(
            "event={} chunk={index} final={} {}",
            event.sequence,
            end == remaining.len(),
            &remaining[..end]
        );
        sink.record(
            Subsystem::Other,
            "kontra-log",
            Phase::Event,
            Context {
                correlation_id: Some(event.sequence),
                instance_id: event.instance_id,
            },
            detail,
        );
        remaining = &remaining[end..];
        index += 1;
    }
}

fn sanitize_automatic(value: &mut serde_json::Value) {
    crate::diagnostics::clean(value, true);
    match value {
        serde_json::Value::Object(map) => {
            // Library source belongs to its author. Location metadata remains, source excerpts stay local.
            map.retain(|key, _| {
                let lower = key.to_ascii_lowercase();
                !lower.contains("excerpt")
                    && !matches!(
                        lower.as_str(),
                        "authorization"
                            | "license_key"
                            | "crashreporterkey"
                            | "anonymousdiagnosticid"
                            | "userid"
                    )
            });
            for value in map.values_mut() {
                sanitize_automatic(value);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                sanitize_automatic(value);
            }
        }
        _ => {}
    }
}

fn request_agent() -> Result<ureq::Agent, String> {
    if cfg!(test) || std::env::var_os("KONTRA_DISABLE_NETWORK").is_some() {
        return Err("Network access is disabled; the crash report remains on disk.".into());
    }
    Ok(ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .https_only(true)
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(std::time::Duration::from_secs(8)))
            .timeout_connect(Some(std::time::Duration::from_secs(3)))
            .timeout_recv_body(Some(std::time::Duration::from_secs(5)))
            .user_agent(format!("KONTRA/{}", env!("CARGO_PKG_VERSION")))
            .build(),
    ))
}
fn read_response(mut response: ureq::http::Response<ureq::Body>) -> Result<(u16, String), String> {
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(64 * 1024 + 1)
        .read_to_string()
        .map_err(|_| "The server response could not be read or was too large".to_string())?;
    Ok((status, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_unicode_events_survive_record_detail_limits_and_are_reconstructable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.dfr");
        let recorder = derpcat_flight_recorder::Recorder::start(path.clone(), 512, 0).unwrap();
        let event: crate::diagnostics::LogEvent = serde_json::from_value(serde_json::json!({
            "schema_version":1,"sequence":42,"timestamp_ms":1,"monotonic_ms":1,"session_id":"test",
            "level":"warning","module":"loader","event":"failure","data":{"message":"é😀".repeat(500),"excerpt":{"text":"commercial code"},"line":22}
        })).unwrap();
        let mut expected = serde_json::to_value(&event).unwrap();
        sanitize_automatic(&mut expected);
        mirror_event(&recorder.sink(), &event);
        assert!(recorder.flush(std::time::Duration::from_secs(2)));
        let rows = derpcat_flight_recorder::read_journal(&path, usize::MAX);
        let rows: Vec<_> = rows
            .iter()
            .filter(|row| row.action == "kontra-log")
            .collect();
        assert!(rows.len() > 5);
        let reconstructed: String = rows
            .iter()
            .map(|row| row.detail.splitn(4, ' ').nth(3).unwrap())
            .collect();
        assert_eq!(reconstructed, expected.to_string());
        assert!(!reconstructed.contains("commercial code"));
        assert!(recorder.shutdown(std::time::Duration::from_secs(2)));
    }
    #[test]
    fn automatic_reports_keep_fault_location_but_no_library_source_or_credentials() {
        let mut value = serde_json::json!({"script_slot":2,"line":9,"column":5,"excerpt":{"text":"proprietary"},"nested":{"script_source":"secret","access_key":"key","path":"/home/user/library.nki"}});
        sanitize_automatic(&mut value);
        assert_eq!(value["line"], 9);
        assert_eq!(value["script_slot"], 2);
        assert!(!value.to_string().contains("proprietary"));
        assert!(!value.to_string().contains("secret"));
        assert!(!value.to_string().contains("/home/user"));
        assert!(
            request_agent().is_err(),
            "test builds must never open network sockets"
        );
    }
}
