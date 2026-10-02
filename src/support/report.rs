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
    error: Option<String>,
}

fn attach_incident_provenance(payload: &mut ReportPayload, incident: &CrashIncident) {
    payload.incident_kind = Some(incident.kind_label().to_owned());
    payload.crash_fingerprint = incident.crash_fingerprint();
    payload.incident_version = Some(incident.version.clone());
    payload.incident_build_id = Some(incident.build_id().to_owned());
}

pub(super) fn try_auto_report_pending_incident() {
    let Some(incident) = pending_incident() else {
        return;
    };
    if AUTOMATIC_CRASH_REPORT_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let started = crash::spawn_detached("kontra-support-report", move || {
        // Delayed native artifacts are searched only on this worker, never during host initialization.
        let incident = crash::refresh_pending_incident(&incident.id).unwrap_or(incident);
        if !incident.auto_reportable() {
            AUTOMATIC_CRASH_REPORT_STARTED.store(false, Ordering::Release);
            return;
        }
        let identity = HOST_IDENTITY.lock_unpoisoned().clone();
        let platform = platform::snapshot();
        let diagnostics = redact_log(&crash::complete_diagnostics(&incident.id));
        let mut payload = ReportPayload {
            schema: 3, product: "kontra", version: crate::build_info::BUILD.version,
            build_id: crate::build_info::BUILD.build_hash,
            incident_id: Some(incident.id.clone()), incident_kind: None, incident_version: None,
            incident_build_id: None, crash_fingerprint: None,
            os_name: platform.os_name.clone(), os_version: platform.os_version.clone(),
            arch: platform.process_architecture.clone(), host_name: identity.0, plugin_format: identity.1,
            description: "KONTRA closed unexpectedly during the previous host session. This report was sent automatically after KONTRA reloaded.".into(),
            diagnostics_attached: true, diagnostics: Some(bounded_diagnostics(diagnostics.clone())),
            diagnostics_full: Some(diagnostics), machine_hashes: Vec::new(),
        };
        attach_incident_provenance(&mut payload, &incident);
        let result = send_report(payload);
        let (level, status) = match &result {
            Ok(message) => (crate::diagnostics::LogLevel::Info, message.clone()),
            Err(error) => {
                AUTOMATIC_CRASH_REPORT_STARTED.store(false, Ordering::Release);
                (
                    crate::diagnostics::LogLevel::Error,
                    format!("Crash report retained for retry: {error}"),
                )
            }
        };
        // Receipt or failure remains visible locally; pending evidence is deleted only after acknowledgement.
        let status_path = support_cache_path().with_file_name("last-report.json");
        let value =
            serde_json::json!({"incident_id":incident.id,"status":status,"sent":result.is_ok()});
        if let Ok(bytes) = serde_json::to_vec(&value) {
            let _ = buffr_durable_file::publish_private(&status_path, &bytes);
        }
        crate::diagnostics::event(level, "support", "automatic_crash_report", value);
    });
    if let Err(error) = started {
        AUTOMATIC_CRASH_REPORT_STARTED.store(false, Ordering::Release);
        crate::diagnostics::event(
            crate::diagnostics::LogLevel::Error,
            "support",
            "report_worker_failed",
            serde_json::json!({"error":error.to_string()}),
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
    let looks_absolute = token.starts_with('/')
        || lower.contains("/users/")
        || lower.contains("\\users\\")
        || lower.contains(":\\");
    if !looks_absolute {
        return token.to_string();
    }
    dependency_suffix(token).unwrap_or_else(|| "[redacted]".to_string())
}

fn send_report(payload: ReportPayload) -> Result<String, String> {
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
    if let Some(incident_id) = payload.incident_id.as_deref() {
        crash::mark_submitted(incident_id);
    }
    Ok(format!("Report {report_id} sent. Thank you."))
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
