// Adapted from BUFFR fd2fdba92f3f71cee24c0c72a39aa190fc9ee414; ISC, see LICENSE-BUFFR.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ReportPayload {
    schema: u8,
    product: &'static str,
    version: String,
    build_id: String,
    incident_id: Option<String>,
    incident_kind: Option<String>,
    incident_version: Option<String>,
    incident_build_id: Option<String>,
    crash_fingerprint: Option<String>,
    os_name: String,
    os_version: String,
    arch: String,
    host_name: String,
    plugin_format: String,
    description: String,
    diagnostics_attached: bool,
    diagnostics: Option<String>,
    diagnostics_full: Option<String>,
    machine_hashes: Vec<[u8; 32]>,
    #[serde(skip)]
    incident_metadata: String,
    #[serde(skip)]
    reporter_context: String,
}
#[derive(Deserialize)]
struct ReportResponse {
    diagnostics_sha256: Option<String>,
    ok: Option<bool>,
    report_id: Option<String>,
    issue_url: Option<String>,
    error: Option<String>,
}

struct DeliveredReport {
    report_id: String,
    issue_url: Option<String>,
}

fn public_issue_url(url: &str) -> Option<String> {
    let number = url.strip_prefix("https://github.com/DerpcatMusic/KONTRA/issues/")?;
    (number.len() <= 20
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && number.bytes().any(|b| b != b'0'))
    .then(|| url.to_owned())
}

fn report_status(incident_id: &str, result: &Result<DeliveredReport, String>) -> serde_json::Value {
    let (status, report_id, issue_url) = match result {
        Ok(delivery) => {
            let mut status = format!("Report {} sent. Thank you.", delivery.report_id);
            if let Some(url) = &delivery.issue_url {
                status.push_str(&format!(" Issue: {url}"));
            }
            (
                status,
                Some(delivery.report_id.clone()),
                delivery.issue_url.clone(),
            )
        }
        Err(error) => (
            format!("Crash report retained for retry: {error}"),
            None,
            None,
        ),
    };
    serde_json::json!({"incident_id":incident_id,"status":status,"reason":status,
        "sent":result.is_ok(),"report_id":report_id,"issue_url":issue_url})
}

fn read_last_report_status(path: &std::path::Path) -> Option<serde_json::Value> {
    use std::io::Read as _;
    const MAX_STATUS_BYTES: usize = 16 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_STATUS_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_STATUS_BYTES {
        return None;
    }
    let saved: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let sent = saved["sent"].as_bool()?;
    let status = redact_log(saved["status"].as_str()?);
    let incident = saved["incident_id"]
        .as_str()
        .filter(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    let report = saved["report_id"].as_str().filter(|s| {
        !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    });
    let issue = saved["issue_url"].as_str().and_then(public_issue_url);
    Some(
        serde_json::json!({"sent":sent,"status":status,"reason":status,
        "incident_id":incident,"report_id":report,"issue_url":issue,"restored":true}),
    )
}

pub(super) fn restore_last_report_status() {
    static RESTORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RESTORED.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(value) =
        read_last_report_status(&support_cache_path().with_file_name("last-report.json"))
    {
        let level = if value["sent"] == true {
            crate::diagnostics::LogLevel::Info
        } else {
            crate::diagnostics::LogLevel::Error
        };
        crate::diagnostics::event(level, "support", "previous_crash_report", value);
    }
}

fn attach_incident_provenance(payload: &mut ReportPayload, incident: &CrashIncident) {
    payload.incident_kind = Some(incident.kind_label().to_owned());
    payload.crash_fingerprint = incident.crash_fingerprint();
    payload.incident_version = Some(if valid_recorded_version(&incident.version) {
        incident.version.clone()
    } else {
        "unknown".into()
    });
    payload.incident_build_id = Some(recorded_field(incident.build_id()));
    payload.version = if valid_recorded_version(&incident.version) {
        incident.version.clone()
    } else {
        // The schema3 endpoint requires a semantic version. This sentinel is
        // explicit about missing/invalid provenance, never the reopening build.
        "0.0.0-unknown".into()
    };
    payload.build_id = recorded_field(incident.build_id());
    payload.host_name = recorded_field(&incident.host_name);
    payload.plugin_format = recorded_field(&incident.plugin_api);
    payload.os_name = recorded_field(if incident.platform.os_name.trim().is_empty() {
        &incident.os
    } else {
        &incident.platform.os_name
    });
    payload.os_version = recorded_field(&incident.platform.os_version);
    payload.arch = recorded_field(if incident.architecture.trim().is_empty() {
        &incident.platform.process_architecture
    } else {
        &incident.architecture
    });
    // The existing backend parses the FIRST matching metadata label from the
    // preview, without section scope. Emit every canonical label, even when
    // unknown, before any captured logs or separately labeled reporter context.
    payload.incident_metadata = format!(
        "Recorded crash metadata (incident time):\nKONTRA version: {}\nRecorded version: {}\nRecorded build ID: {}\nOperating system: {}\nOS version: {}\nProcess architecture: {}\nHost: {}\nHost process: {}\nPlugin format: {}\n",
        if payload.version == "0.0.0-unknown" {
            "Unknown (missing/invalid recorded version; transport sentinel 0.0.0-unknown)"
        } else {
            &payload.version
        },
        payload.incident_version.as_deref().unwrap_or_default(),
        payload.build_id,
        payload.os_name,
        payload.os_version,
        payload.arch,
        payload.host_name,
        recorded_field(&incident.host_process),
        payload.plugin_format,
    );
}

fn recorded_field(value: &str) -> String {
    // Flatten before labeling: captured host strings must not inject a second
    // metadata line. Bound/redact values without removing their canonical label.
    let truncated = value.chars().nth(256).is_some();
    let value: String = value.chars().take(256).collect();
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        "Unknown (not recorded at incident time)".into()
    } else {
        let mut value = redact_log(&value);
        if truncated {
            value.push_str(" [metadata summary truncated]");
        }
        value
    }
}

fn valid_recorded_version(value: &str) -> bool {
    // Accept bounded versions matching the deployed schema3 PLUGIN_VERSION contract.
    if value.len() > 128 {
        return false;
    }
    let (core, suffix) = value
        .split_once('-')
        .map_or((value, None), |(core, suffix)| (core, Some(suffix)));
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && suffix.is_none_or(|suffix| {
            !suffix.is_empty()
                && suffix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        })
}

fn set_report_diagnostics(payload: &mut ReportPayload, complete: &str) {
    let diagnostics = format!(
        "{}\n{}\n\n{}",
        payload.incident_metadata,
        redact_log(complete),
        payload.reporter_context
    );
    payload.diagnostics = Some(bounded_diagnostics(diagnostics.clone()));
    payload.diagnostics_full = Some(diagnostics);
}

fn automatic_report_payload(
    incident: &CrashIncident,
    current_identity: &(String, String),
    current_platform: &platform::PlatformSnapshot,
    complete: &str,
) -> ReportPayload {
    let mut payload = ReportPayload {
        schema: 3, product: "kontra", version: String::new(), build_id: String::new(),
        incident_id: Some(incident.id.clone()), incident_kind: None, incident_version: None,
        incident_build_id: None, crash_fingerprint: None,
        os_name: String::new(), os_version: String::new(), arch: String::new(),
        host_name: String::new(), plugin_format: String::new(),
        description: "The previous host session ended with a confirmed crash while KONTRA was loaded. Fault attribution is unknown unless the attached exception and stack establish it. This report was sent automatically after KONTRA reloaded.".into(),
        diagnostics_attached: true, diagnostics: None, diagnostics_full: None, machine_hashes: Vec::new(),
        incident_metadata: String::new(),
        reporter_context: format!(
            "Reporter context (reopening session; not crash attribution):\nReporter KONTRA version: {}\nReporter build ID: {}\nReporter host: {}\nReporter plugin format: {}\n{}",
            crate::build_info::BUILD.version, crate::build_info::BUILD.build_hash,
            recorded_field(&current_identity.0).replace("not recorded at incident time", "not available in reopening session"),
            recorded_field(&current_identity.1).replace("not recorded at incident time", "not available in reopening session"),
            current_platform.render().lines().map(|line| format!("Reporter {line}")).collect::<Vec<_>>().join("\n"),
        ),
    };
    payload.reporter_context = redact_log(&payload.reporter_context);
    attach_incident_provenance(&mut payload, incident);
    set_report_diagnostics(&mut payload, complete);
    payload
}

pub(super) struct AutomaticReportPermit<'a>(&'a std::sync::atomic::AtomicBool);

impl<'a> AutomaticReportPermit<'a> {
    fn acquire(active: &'a std::sync::atomic::AtomicBool) -> Option<Self> {
        (!active.swap(true, Ordering::AcqRel)).then(|| Self(active))
    }
}

impl Drop for AutomaticReportPermit<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct AutomaticReportCompletion(Option<String>);

impl Drop for AutomaticReportCompletion {
    fn drop(&mut self) {
        if let Some(incident_id) = self.0.take() {
            crash::continue_automatic_reports(incident_id);
        }
    }
}

pub(super) fn try_auto_report_pending_incident() {
    let Some(incident) = pending_incident() else {
        return;
    };
    let Some(permit) = AutomaticReportPermit::acquire(&AUTOMATIC_CRASH_REPORT_STARTED) else {
        return;
    };
    let started = crash::spawn_detached("kontra-support-report", move || {
        // The module remains pinned after delivery; release the upload gate when
        // this worker finishes so a later incident in the same host can report.
        let mut completion = AutomaticReportCompletion(None);
        let _permit = permit;
        // Delayed native artifacts are searched only on this worker, never during host initialization.
        let incident = crash::refresh_pending_incident(&incident.id).unwrap_or(incident);
        if !incident.auto_reportable() {
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Warning,
                "support",
                "previous_session_unconfirmed",
                serde_json::json!({"incident_id":incident.id,
                    "reason":"A previous session ended without native crash confirmation. Its evidence remains local and has not been sent."}),
            );
            return;
        }
        let identity = HOST_IDENTITY.lock_unpoisoned().clone();
        let platform = platform::snapshot(&std::sync::atomic::AtomicBool::new(false));
        let payload = automatic_report_payload(
            &incident,
            &identity,
            platform,
            &crash::complete_diagnostics(&incident.id),
        );
        let result = send_report(payload);
        let level = if result.is_ok() {
            crate::diagnostics::LogLevel::Info
        } else {
            crate::diagnostics::LogLevel::Error
        };
        // Receipt or failure remains visible locally; pending evidence is deleted only after acknowledgement.
        let status_path = support_cache_path().with_file_name("last-report.json");
        let value = report_status(&incident.id, &result);
        if let Ok(bytes) = serde_json::to_vec(&value) {
            if let Err(error) = buffr_durable_file::publish_private_streaming(
                &status_path,
                std::time::Duration::from_millis(500),
                |file| std::io::Write::write_all(file, &bytes),
            ) {
                crate::diagnostics::event(
                    crate::diagnostics::LogLevel::Error,
                    "support",
                    "report_receipt_write_failed",
                    serde_json::json!({"reason":format!("Could not persist the crash-report delivery status: {error}")}),
                );
            }
        }
        crate::diagnostics::event(level, "support", "automatic_crash_report", value);
        if result.is_ok() {
            completion.0 = Some(incident.id);
        }
        // All delivery/receipt work finishes before the permit drops; only then
        // does the completion guard enqueue continuation on the reporter worker.
    });
    if let Err(error) = started {
        crate::diagnostics::event(
            crate::diagnostics::LogLevel::Error,
            "support",
            "report_worker_failed",
            serde_json::json!({"reason":format!("Crash report retained; could not start delivery worker: {error}")}),
        );
    }
}

#[cfg(test)]
pub(super) fn test_detached_automatic_delivery(
    incident_id: String,
    response: Result<(u16, String), String>,
    entered: std::sync::mpsc::Sender<()>,
    outcome: std::sync::mpsc::Sender<bool>,
    release: std::sync::mpsc::Receiver<()>,
) -> Result<(), String> {
    let permit = AutomaticReportPermit::acquire(&AUTOMATIC_CRASH_REPORT_STARTED)
        .ok_or_else(|| "automatic report worker is busy".to_string())?;
    crash::spawn_detached("kontra-mocked-automatic-delivery", move || {
        let mut completion = AutomaticReportCompletion(None);
        let _permit = permit;
        entered.send(()).unwrap();
        let acknowledged = response
            .and_then(|(status, text)| {
                validate_receipt(status, &text, Some("authored-full-evidence".into()))
            })
            .is_ok();
        if acknowledged {
            crash::mark_submitted(&incident_id);
            completion.0 = Some(incident_id);
        }
        outcome.send(acknowledged).unwrap();
        release
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap();
    })
    .map_err(|error| error.to_string())
}

#[cfg(test)]
pub(super) fn test_acquire_automatic_report_permit() -> Option<AutomaticReportPermit<'static>> {
    AutomaticReportPermit::acquire(&AUTOMATIC_CRASH_REPORT_STARTED)
}

#[cfg(test)]
pub(super) fn test_automatic_report_is_busy() -> bool {
    AUTOMATIC_CRASH_REPORT_STARTED.load(Ordering::Acquire)
}

pub fn redact_log(log: &str) -> String {
    log.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "license key",
                "license_key",
                "activation request",
                "machine id",
                "authorization:",
                "bearer ",
                "dla1.",
            ]
            .iter()
            .any(|marker| lower.contains(marker))
            {
                return "[redacted sensitive diagnostic]".to_string();
            }
            line.split_whitespace()
                .map(redact_token)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bounded_diagnostics(text: String) -> String {
    const LIMIT: usize = 32_000;
    const HEAD: usize = 8_000;
    const OMITTED: &str = "\n[older diagnostics omitted]\n";
    const TAIL: usize = LIMIT - HEAD - OMITTED.len();
    // The char after the last kept tail char; `None` means the text fits.
    let Some((cut, cut_char)) = text.char_indices().rev().nth(TAIL) else {
        return text;
    };
    if text.char_indices().nth(LIMIT).is_none() {
        return text;
    }
    let head_end = text
        .char_indices()
        .nth(HEAD)
        .map_or(text.len(), |(index, _)| index);
    format!(
        "{}{OMITTED}{}",
        &text[..head_end],
        &text[cut + cut_char.len_utf8()..]
    )
}

const DEPENDENCY_ROOTS: [(&str, usize); 4] = [
    ("/registry/src/", 1),
    ("/.cargo/git/checkouts/", 0),
    ("/rustc/", 1),
    ("/.rustup/toolchains/", 1),
];

/// The part of an absolute path that names code rather than the machine, or
/// `None` when nothing about it is safe to keep.
///
/// A dependency panic location is the whole value of a crash report, and
/// blanking it wholesale because it happens to be absolute makes the report
/// untriageable. The home prefix goes; `crate-version/src/file.rs:line:col`
/// stays.
fn dependency_suffix(token: &str) -> Option<String> {
    // `to_ascii_lowercase` and this replacement are both byte-length
    // preserving, so offsets found here index the original token.
    let normalized = token.to_ascii_lowercase().replace('\\', "/");
    let (root, skip) = DEPENDENCY_ROOTS
        .iter()
        .find_map(|(root, skip)| normalized.find(root).map(|at| (at + root.len(), *skip)))?;
    let mut rest = &token[root..];
    for _ in 0..skip {
        rest = rest.split_once(['/', '\\'])?.1;
    }
    (!rest.is_empty()).then(|| rest.replace('\\', "/"))
}

fn redact_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if token.contains('@') {
        return "[redacted]".to_string();
    }
    let unquoted = token.trim_start_matches(['\'', '"', '(', '[', '{']);
    let windows_drive = token.as_bytes().windows(3).enumerate().any(|(at, bytes)| {
        bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\')
            && (at == 0 || matches!(token.as_bytes()[at - 1], b'=' | b'\'' | b'"' | b'(' | b'['))
    });
    let looks_absolute = unquoted.starts_with('/')
        || unquoted.starts_with("\\\\")
        || token.contains("=/")
        || token.contains("=\\\\")
        || token.contains("=\"/")
        || token.contains("\":\"/")
        || windows_drive
        || lower.contains("/users/")
        || lower.contains("\\users\\")
        || lower.contains(":\\");
    if !looks_absolute {
        return token.to_string();
    }
    dependency_suffix(token).unwrap_or_else(|| "[redacted]".to_string())
}

// The service limit applies to serialized UTF-8 JSON, including escape expansion
// and the preview. Stop accumulating serialized bytes at that limit before a
// request; pending evidence is retained without an acknowledgement.
fn bounded_report_body(value: &impl serde::Serialize, limit: usize) -> Result<Vec<u8>, String> {
    bounded_json(value, limit, false)
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn bounded_pretty_json(value: &impl serde::Serialize, limit: usize) -> Option<String> {
    String::from_utf8(bounded_json(value, limit, true).ok()?).ok()
}

fn bounded_json(
    value: &impl serde::Serialize,
    limit: usize,
    pretty: bool,
) -> Result<Vec<u8>, String> {
    struct Body {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Body {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other(
                    "The complete report exceeds the 16 MiB JSON upload limit. It remains local; export it for support.",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut body = Body {
        bytes: Vec::new(),
        limit,
    };
    if pretty {
        serde_json::to_writer_pretty(&mut body, value).map_err(|error| error.to_string())?;
    } else {
        serde_json::to_writer(&mut body, value).map_err(|error| error.to_string())?;
    }
    Ok(body.bytes)
}

fn send_report(payload: ReportPayload) -> Result<DeliveredReport, String> {
    let mut payload = payload;
    if let Some(incident_id) = payload.incident_id.as_deref()
        && let Some(incident) = crash::refresh_pending_incident(incident_id)
    {
        attach_incident_provenance(&mut payload, &incident);
    }
    if payload.diagnostics_full.is_some()
        && let Some(incident_id) = payload.incident_id.as_deref()
    {
        // Keep retries independent of whichever new session recovers the incident.
        let complete = crash::complete_diagnostics(incident_id);
        if complete.trim().is_empty() {
            return Err(
                "The incident evidence is unavailable; it has not been marked sent.".to_string(),
            );
        }
        set_report_diagnostics(&mut payload, &complete);
    }
    payload.machine_hashes = Vec::new();
    use sha2::Digest as _;
    let evidence_hash = payload
        .diagnostics_full
        .as_ref()
        .map(|text| format!("{:x}", sha2::Sha256::digest(text.as_bytes())));
    let body = bounded_report_body(&payload, 16 * 1024 * 1024)?;
    let (status, text) = request_agent()?
        .post(REPORT_URL)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .send(body)
        .map_err(|_| "Could not reach support. Check your connection and try again.".to_string())
        .and_then(read_response)?;
    let report_id = validate_receipt(status, &text, evidence_hash)?;
    let issue_url = serde_json::from_str::<ReportResponse>(&text)
        .ok()
        .and_then(|response| response.issue_url)
        .as_deref()
        .and_then(public_issue_url);
    if let Some(incident_id) = payload.incident_id.as_deref() {
        crash::mark_submitted(incident_id);
    }
    Ok(DeliveredReport {
        report_id,
        issue_url,
    })
}

fn validate_receipt(
    status: u16,
    text: &str,
    evidence_hash: Option<String>,
) -> Result<String, String> {
    let response: ReportResponse = serde_json::from_str(text).unwrap_or(ReportResponse {
        diagnostics_sha256: None,
        ok: None,
        report_id: None,
        issue_url: None,
        error: None,
    });
    if !(200..300).contains(&status) {
        return Err(response.error.unwrap_or_else(|| {
            "Support could not accept the report. Try again later.".to_string()
        }));
    }
    let report_id = response
        .report_id
        .filter(|id| response.ok == Some(true) && !id.trim().is_empty())
        .ok_or_else(|| "Support did not confirm the report. Try again.".to_string())?;
    if evidence_hash.is_some() && response.diagnostics_sha256 != evidence_hash {
        return Err(
            "Support did not confirm the complete diagnostics. The incident is retained for retry."
                .to_string(),
        );
    }
    Ok(report_id)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_crash_payload_uses_incident_host_instead_of_reopening_host() {
        let mut incident = crash::test_incident("0.3.64", "recorded-build");
        incident.host_name = "Studio One 7.2".into();
        incident.host_process = "Studio One".into();
        incident.plugin_api = "VST3".into();
        incident.os = "macos".into();
        incident.architecture = "aarch64".into();
        incident.platform = platform::PlatformSnapshot {
            os_name: "macOS".into(),
            os_version: "15.6".into(),
            process_architecture: "aarch64".into(),
            ..Default::default()
        };
        let mut saved = serde_json::to_value(&incident).unwrap();
        saved["kind"] = serde_json::json!("platform_crash");
        incident = serde_json::from_value(saved).unwrap();
        assert!(incident.auto_reportable());
        let reopening = platform::PlatformSnapshot {
            os_name: "Windows".into(),
            os_version: "11".into(),
            process_architecture: "x86_64".into(),
            ..Default::default()
        };
        // These unscoped labels occur in real captured/current diagnostics. They
        // must never win the backend's first-label public-summary parser.
        let logs = format!(
            "Host: REAPER 7.5\nOperating system: Windows\nOS version: 11\nProcess architecture: x86_64\nPlugin format: CLAP\n{}",
            "diagnostic line\n".repeat(4_000)
        );
        let payload = automatic_report_payload(
            &incident,
            &("REAPER 7.5".into(), "CLAP".into()),
            &reopening,
            &logs,
        );
        let wire = serde_json::to_value(&payload).unwrap();
        assert_eq!(wire["version"], "0.3.64");
        assert_eq!(wire["incident_version"], "0.3.64");
        assert_eq!(wire["build_id"], "recorded-build");
        assert_eq!(wire["host_name"], "Studio One 7.2");
        assert_eq!(wire["plugin_format"], "VST3");
        assert_eq!(wire["os_name"], "macOS");
        assert_eq!(wire["os_version"], "15.6");
        assert_eq!(wire["arch"], "aarch64");
        for field in ["diagnostics", "diagnostics_full"] {
            let diagnostics = wire[field].as_str().unwrap();
            assert!(diagnostics.starts_with("Recorded crash metadata (incident time):\n"));
            for (label, expected) in [
                ("Host: ", "Studio One 7.2"),
                ("Host process: ", "Studio One"),
                ("Plugin format: ", "VST3"),
                ("Operating system: ", "macOS"),
                ("OS version: ", "15.6"),
                ("Process architecture: ", "aarch64"),
            ] {
                assert_eq!(
                    diagnostics
                        .lines()
                        .find_map(|line| line.strip_prefix(label)),
                    Some(expected)
                );
            }
        }
        let full = wire["diagnostics_full"].as_str().unwrap();
        assert!(full.contains("Reporter host: REAPER 7.5\nReporter plugin format: CLAP"));
        assert!(full.contains("Reporter Operating system: Windows"));
        assert!(full.contains(&format!(
            "Reporter KONTRA version: {}",
            crate::build_info::BUILD.version
        )));
        assert!(full.contains(&format!(
            "Reporter build ID: {}",
            crate::build_info::BUILD.build_hash
        )));
        assert!(wire.get("incident_metadata").is_none() && wire.get("reporter_context").is_none());
        assert!(wire["diagnostics"].as_str().unwrap().chars().count() <= 32_000);
    }

    #[test]
    fn missing_recorded_metadata_stays_unknown_and_never_borrows_current_context() {
        let mut incident = crash::test_incident("", "");
        incident.host_name.clear();
        incident.host_process.clear();
        incident.plugin_api.clear();
        incident.os.clear();
        incident.architecture.clear();
        let reopening = platform::PlatformSnapshot {
            os_name: "Windows".into(),
            os_version: "11".into(),
            process_architecture: "x86_64".into(),
            ..Default::default()
        };
        for version in [
            "",
            "nightly",
            "1.2",
            "1.2.3+private",
            "1.2.3-",
            "1.2.3\nHost: REAPER",
        ] {
            incident.version = version.into();
            let payload = automatic_report_payload(
                &incident,
                &("REAPER".into(), "CLAP".into()),
                &reopening,
                "Host: REAPER\nPlugin format: CLAP\nOperating system: Windows",
            );
            assert_eq!(payload.version, "0.0.0-unknown");
            assert_eq!(payload.incident_version.as_deref(), Some("unknown"));
            for field in [
                &payload.host_name,
                &payload.plugin_format,
                &payload.os_name,
                &payload.os_version,
                &payload.arch,
            ] {
                assert_eq!(field, "Unknown (not recorded at incident time)");
            }
            let preview = payload.diagnostics.as_ref().unwrap();
            assert!(preview.contains("KONTRA version: Unknown (missing/invalid recorded version; transport sentinel 0.0.0-unknown)"));
            for label in [
                "Host: ",
                "Host process: ",
                "Plugin format: ",
                "Operating system: ",
                "OS version: ",
                "Process architecture: ",
            ] {
                assert_eq!(
                    preview.lines().find_map(|line| line.strip_prefix(label)),
                    Some("Unknown (not recorded at incident time)")
                );
            }
        }
        for version in ["0.3.64", "1.2.3-rc.1", "0.0.0-unknown"] {
            assert!(valid_recorded_version(version));
        }
        // A missing display name does not borrow even a known *recorded* process
        // name; that name remains separately available to interpret the evidence.
        incident.host_process = "Studio One".into();
        let payload =
            automatic_report_payload(&incident, &("REAPER".into(), "CLAP".into()), &reopening, "");
        assert!(payload.host_name.starts_with("Unknown"));
        assert!(
            payload
                .diagnostics
                .unwrap()
                .contains("Host process: Studio One")
        );
    }

    #[test]
    fn refreshed_delivery_rebuilds_both_metadata_previews_and_redacts_values() {
        let mut incident = crash::test_incident("0.3.64", "recorded-build");
        let mut payload = automatic_report_payload(
            &incident,
            &("REAPER".into(), "CLAP".into()),
            &Default::default(),
            "initial evidence",
        );
        incident.host_name = "Reason 13\nHost: REAPER".into();
        incident.plugin_api = "VST3".into();
        incident.os = "macos".into();
        incident.architecture = "aarch64".into();
        incident.host_process = "/Users/private/Reason".into();
        // These are the exact shared functions used after send_report refreshes
        // durable incident evidence, before hashing/serializing the final packet.
        attach_incident_provenance(&mut payload, &incident);
        set_report_diagnostics(
            &mut payload,
            "fresh exception and stack\nOperating system: Windows",
        );
        for diagnostics in [
            payload.diagnostics.as_ref().unwrap(),
            payload.diagnostics_full.as_ref().unwrap(),
        ] {
            assert!(diagnostics.contains("Host: Reason 13 Host: REAPER\n"));
            assert!(diagnostics.contains("Plugin format: VST3\n"));
            assert!(diagnostics.contains("Operating system: macos\n"));
            assert!(diagnostics.contains("Process architecture: aarch64\n"));
            assert!(diagnostics.contains("Host process: [redacted]\n"));
            assert!(diagnostics.contains("fresh exception and stack"));
            assert!(
                !diagnostics.contains("initial evidence")
                    && !diagnostics.contains("/Users/private")
            );
        }
        assert_eq!(payload.os_name, "macos");
        assert_eq!(payload.arch, "aarch64");
        assert_eq!(
            recorded_field("bearer secret"),
            "[redacted sensitive diagnostic]"
        );
        let bounded = recorded_field(&"😀".repeat(1_000));
        assert!(bounded.ends_with(" [metadata summary truncated]"));
        assert_eq!(bounded.chars().filter(|&c| c == '😀').count(), 256);
    }
    #[test]
    fn delivery_status_restores_a_readable_receipt_without_untrusted_fields() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("last-report.json");
        let url = "https://github.com/DerpcatMusic/KONTRA/issues/123";
        let delivered = Ok(DeliveredReport {
            report_id: "report-123".into(),
            issue_url: Some(url.into()),
        });
        let mut status = report_status("0123456789abcdef", &delivered);
        assert!(
            status["reason"]
                .as_str()
                .unwrap()
                .contains("Report report-123 sent")
        );
        assert!(status["reason"].as_str().unwrap().contains(url));
        status["license_key"] = serde_json::json!("must not restore");
        buffr_durable_file::publish_private(&path, &serde_json::to_vec(&status).unwrap()).unwrap();
        let restored = read_last_report_status(&path).unwrap();
        assert_eq!(restored["report_id"], "report-123");
        assert_eq!(restored["issue_url"], url);
        assert_eq!(restored["reason"], restored["status"]);
        assert_eq!(restored["restored"], true);
        assert!(restored.get("license_key").is_none());
        let failed = report_status("0123456789abcdef", &Err("offline".into()));
        assert_eq!(failed["sent"], false);
        assert!(
            failed["reason"]
                .as_str()
                .unwrap()
                .contains("retained for retry")
        );
        std::fs::write(&path, serde_json::to_vec(&failed).unwrap()).unwrap();
        assert_eq!(read_last_report_status(&path).unwrap()["sent"], false);
        std::fs::write(&path, b"{}").unwrap();
        assert!(read_last_report_status(&path).is_none());
        std::fs::write(&path, vec![b' '; 16 * 1024 + 1]).unwrap();
        assert!(read_last_report_status(&path).is_none());
    }

    #[test]
    fn public_issue_links_reject_private_or_foreign_urls() {
        assert!(public_issue_url("https://github.com/DerpcatMusic/KONTRA/issues/42").is_some());
        for url in [
            "https://github.com/DerpcatMusic/buffr-support/issues/42",
            "https://github.com/DerpcatMusic/KONTRA/issues/0",
            "https://github.com/DerpcatMusic/KONTRA/issues/42?token=secret",
            "https://github.com/DerpcatMusic/KONTRA/issues/42#private",
            "https://github.com/DerpcatMusic/KONTRA/issues/42/evil",
            "https://github.com/Other/KONTRA/issues/42",
            "http://github.com/DerpcatMusic/KONTRA/issues/42",
            "https://github.com.evil/DerpcatMusic/KONTRA/issues/42",
        ] {
            assert!(public_issue_url(url).is_none(), "{url}");
        }
    }

    #[test]
    fn upload_limit_counts_complete_serialized_utf8_json_and_escape_expansion() {
        let value = serde_json::json!({"diagnostics_full":"😀\n\"\\", "diagnostics":"preview"});
        let complete = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            bounded_report_body(&value, complete.len()).unwrap(),
            complete
        );
        let error = bounded_report_body(&value, complete.len() - 1).unwrap_err();
        assert!(error.contains("JSON upload limit") && error.contains("remains local"));
        assert!(bounded_report_body(&value, 0).is_err());
    }

    #[test]
    fn native_report_redaction_covers_quoted_paths_and_forward_slash_windows_drives() {
        let evidence = "Binary: \"/Volumes/private/library/KONTRA\"\nAppPath=D:/Projects/alice/KONTRA.exe\nImage=\\\\server\\private\\KONTRA.dll\nException: EXC_BAD_ACCESS\nFrame: kontra_render + 12";
        let clean = redact_log(evidence);
        for private in ["/Volumes/private", "D:/Projects", "server\\private"] {
            assert!(!clean.contains(private), "{clean}");
        }
        assert!(clean.contains("EXC_BAD_ACCESS") && clean.contains("kontra_render + 12"));
        assert_eq!(
            redact_log("https://github.com/DerpcatMusic/KONTRA/issues/42"),
            "https://github.com/DerpcatMusic/KONTRA/issues/42"
        );
    }

    #[test]
    fn completed_workers_allow_the_next_incident_after_ack_or_failure() {
        use std::sync::mpsc;
        let timeout = std::time::Duration::from_secs(5);
        // The production flag is static because detached workers outlive their
        // caller. This one-byte test flag is separate from all reporter globals.
        let active: &'static std::sync::atomic::AtomicBool =
            Box::leak(Box::new(std::sync::atomic::AtomicBool::new(false)));
        for completion in 0..4 {
            let permit = AutomaticReportPermit::acquire(active).unwrap();
            let (entered, began) = mpsc::channel();
            let (deliver, delivery) = mpsc::channel();
            let (outcome, result) = mpsc::channel();
            let (finish, finishing) = mpsc::channel();
            let (finished, done) = mpsc::channel::<()>();
            crash::spawn_detached("automatic-report-permit-test", move || {
                // Locals drop in reverse order: closing `finished` proves the
                // captured permit has dropped after this worker's body exits.
                let _finished = finished;
                let _permit = permit;
                entered.send(()).unwrap();
                delivery.recv_timeout(timeout).unwrap();
                let receipt =
                    r#"{"ok":true,"report_id":"first","diagnostics_sha256":"full-evidence"}"#;
                let acknowledged = match completion {
                    0 => validate_receipt(200, receipt, Some("full-evidence".into())).is_ok(),
                    1 => validate_receipt(200, receipt, Some("wrong-sha".into())).is_ok(),
                    2 => request_agent().is_ok(), // Network is disabled in tests.
                    _ => crash::test_incident("0.3.115", "test").auto_reportable(),
                };
                outcome.send(acknowledged).unwrap();
                finishing.recv_timeout(timeout).unwrap();
                if !acknowledged {
                    return; // Rejected receipt, offline or unconfirmed incident.
                }
                // Successful completion leaves through the same scope boundary.
            })
            .unwrap();
            began.recv_timeout(timeout).unwrap();
            assert!(AutomaticReportPermit::acquire(active).is_none());
            deliver.send(()).unwrap();
            assert_eq!(result.recv_timeout(timeout).unwrap(), completion == 0);
            assert!(
                AutomaticReportPermit::acquire(active).is_none(),
                "delivery outcome must not release a still-running worker"
            );
            finish.send(()).unwrap();
            assert_eq!(
                done.recv_timeout(timeout),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
            assert!(!active.load(Ordering::Acquire));
        }
        assert!(AutomaticReportPermit::acquire(active).is_some());
    }

    #[test]
    fn delivery_requires_success_id_and_complete_evidence_hash() {
        let valid = r#"{"ok":true,"report_id":"abc","diagnostics_sha256":"sha"}"#;
        assert_eq!(
            validate_receipt(200, valid, Some("sha".into())).unwrap(),
            "abc"
        );
        for (status, receipt, hash) in [
            (200, valid, "other"),
            (500, valid, "sha"),
            (200, "{}", "sha"),
            (200, r#"{"ok":true,"report_id":"abc"}"#, "sha"),
            (
                200,
                r#"{"ok":false,"report_id":"abc","diagnostics_sha256":"sha"}"#,
                "sha",
            ),
        ] {
            assert!(validate_receipt(status, receipt, Some(hash.into())).is_err());
        }
        assert!(request_agent().is_err());
    }
}
