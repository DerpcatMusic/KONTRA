// Adapted from BUFFR fd2fdba92f3f71cee24c0c72a39aa190fc9ee414; ISC, see LICENSE-BUFFR.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct ReportPayload {
    schema: u8,
    product: &'static str,
    version: &'static str,
    build_id: &'static str,
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
    payload.incident_version = Some(incident.version.clone());
    payload.incident_build_id = Some(incident.build_id().to_owned());
}

struct AutomaticReportPermit<'a>(&'a std::sync::atomic::AtomicBool);

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
        let diagnostics = redact_log(&crash::complete_diagnostics(&incident.id));
        let mut payload = ReportPayload {
            schema: 3, product: "kontra", version: crate::build_info::BUILD.version,
            build_id: crate::build_info::BUILD.build_hash,
            incident_id: Some(incident.id.clone()), incident_kind: None, incident_version: None,
            incident_build_id: None, crash_fingerprint: None,
            os_name: platform.os_name.clone(), os_version: platform.os_version.clone(),
            arch: platform.process_architecture.clone(), host_name: identity.0, plugin_format: identity.1,
            description: "The previous host session ended with a confirmed crash while KONTRA was loaded. Fault attribution is unknown unless the attached exception and stack establish it. This report was sent automatically after KONTRA reloaded.".into(),
            diagnostics_attached: true, diagnostics: Some(bounded_diagnostics(diagnostics.clone())),
            diagnostics_full: Some(diagnostics), machine_hashes: Vec::new(),
        };
        attach_incident_provenance(&mut payload, &incident);
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
            if let Err(error) = buffr_durable_file::publish_private(&status_path, &bytes) {
                crate::diagnostics::event(
                    crate::diagnostics::LogLevel::Error,
                    "support",
                    "report_receipt_write_failed",
                    serde_json::json!({"reason":format!("Could not persist the crash-report delivery status: {error}")}),
                );
            }
        }
        crate::diagnostics::event(level, "support", "automatic_crash_report", value);
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
        payload.diagnostics_full = Some(redact_log(&complete));
    }
    payload.machine_hashes = Vec::new();
    use sha2::Digest as _;
    let evidence_hash = payload
        .diagnostics_full
        .as_ref()
        .map(|text| format!("{:x}", sha2::Sha256::digest(text.as_bytes())));
    let body = serde_json::to_string(&payload).map_err(|error| error.to_string())?;
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
