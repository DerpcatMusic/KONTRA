//! Exact private evidence is included only in a user-requested local export.
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

const FILE_LIMIT: usize = 4096;
const METADATA_LIMIT: u64 = 8 << 20;
// Markers store a process identifier (normally an executable basename), not
// arbitrary report text. Keep even unusually long UTF-8 names within one page.
const HOST_PROCESS_LIMIT: usize = 4096;

pub(crate) struct CrashExport {
    pub manifest: Value,
    pub warnings: Vec<String>,
}

pub(crate) fn export_crash_evidence(
    destination: &Path,
    stopping: &AtomicBool,
) -> std::io::Result<CrashExport> {
    export_from(
        super::support_cache_path()
            .parent()
            .unwrap_or(Path::new("")),
        destination,
        stopping,
    )
}

fn stopped(stopping: &AtomicBool) -> std::io::Result<()> {
    if stopping.load(Ordering::Acquire) {
        Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "Support export canceled",
        ))
    } else {
        Ok(())
    }
}
fn hex(name: &str, length: usize) -> bool {
    name.len() == length && name.bytes().all(|b| b.is_ascii_hexdigit())
}
fn session_journal(name: &str) -> bool {
    name.strip_suffix(".dfr")
        .is_some_and(|stem| !stem.is_empty())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && Path::new(name).components().count() == 1
}
fn plain(path: &Path, directory: bool) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| if directory { m.is_dir() } else { m.is_file() })
}
fn private_new(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}
fn metadata_json(path: &Path) -> std::io::Result<(Value, String)> {
    if !plain(path, false) {
        return Err(std::io::Error::other(
            "Evidence metadata is absent or not a regular file",
        ));
    }
    let file = File::open(path)?;
    if file.metadata()?.len() > METADATA_LIMIT {
        return Err(std::io::Error::other(
            "Evidence metadata exceeds the 8 MiB read limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(METADATA_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > METADATA_LIMIT {
        return Err(std::io::Error::other(
            "Evidence metadata grew past the read limit",
        ));
    }
    Ok((
        serde_json::from_slice(&bytes).map_err(std::io::Error::other)?,
        blake3::hash(&bytes).to_hex().to_string(),
    ))
}

fn export_from(
    cache: &Path,
    destination: &Path,
    stopping: &AtomicBool,
) -> std::io::Result<CrashExport> {
    stopped(stopping)?;
    let target = destination.join("crash-evidence");
    // The enclosing report is already create-new. Never place a report among
    // its own inputs or follow a source/cache directory symlink.
    let destination = destination.canonicalize()?;
    if std::fs::symlink_metadata(cache).is_ok() {
        if !plain(cache, true) {
            return Err(std::io::Error::other(
                "Crash evidence cache is not an owned regular directory",
            ));
        }
        if destination.starts_with(cache.canonicalize()?) {
            return Err(std::io::Error::other(
                "Support export destination overlaps its evidence inputs",
            ));
        }
    }
    private_dir(&target)?;
    let root = cache.join("crash-reports");
    if std::fs::symlink_metadata(&root).is_ok() && !plain(&root, true) {
        return Err(std::io::Error::other(
            "Crash evidence root is not an owned regular directory",
        ));
    }
    let mut owners = std::collections::BTreeMap::<PathBuf, (u32, String, PathBuf, String)>::new();
    let mut references = std::collections::BTreeMap::<PathBuf, Value>::new();
    let mut metadata_hashes = std::collections::BTreeMap::<PathBuf, String>::new();
    let mut sources = std::collections::BTreeSet::<PathBuf>::new();
    let mut session_journals = Vec::new();
    let mut entries = Vec::<Value>::new();
    let mut warnings = Vec::new();
    let mut add = |relative: PathBuf| {
        sources.insert(relative);
    };
    for name in [
        "pending.json",
        "pending-cursor.json",
        "deferred-cursor.json",
    ] {
        if std::fs::symlink_metadata(root.join(name)).is_ok() {
            add(PathBuf::from("crash-reports").join(name));
        }
    }
    if std::fs::symlink_metadata(cache.join("last-report.json")).is_ok() {
        add("last-report.json".into());
    }
    // Fixed directories and filename formats prevent a metadata record from
    // turning manual export into an arbitrary filesystem traversal.
    for directory in ["originals", "pending", "deferred", "sessions", "panics"] {
        let path = root.join(directory);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        if !plain(&path, true) {
            warnings.push(format!("{directory}: non-directory or symlink omitted"));
            continue;
        }
        let files = match std::fs::read_dir(&path) {
            Ok(files) => files,
            Err(e) => {
                warnings.push(format!("{directory}: {e}"));
                continue;
            }
        };
        for (index, entry) in files.take(FILE_LIMIT + 1).enumerate() {
            if index == FILE_LIMIT {
                warnings.push(format!(
                    "{directory}: enumeration limit reached; additional entries omitted"
                ));
                break;
            }
            stopped(stopping)?;
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warnings.push(format!("{directory}: {e}"));
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if buffr_durable_file::is_internal_file_name(name) {
                continue;
            }
            if sources.len() >= FILE_LIMIT {
                warnings.push(
                    "Crash evidence file limit reached; additional sources were not exported"
                        .into(),
                );
                break;
            }
            let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
            if directory == "sessions" && extension == "dfr" {
                session_journals.push(PathBuf::from("crash-reports/sessions").join(name));
                continue;
            }
            let allowed = match directory {
                "originals" => {
                    (extension == "dfr" && hex(stem, 16))
                        || (matches!(extension, "raw" | "json") && hex(stem, 64))
                }
                "pending" | "deferred" => extension == "json" && hex(stem, 16),
                "panics" => {
                    extension == "json"
                        && !stem.is_empty()
                        && stem.bytes().all(|b| b.is_ascii_digit())
                }
                "sessions" => extension == "json",
                _ => false,
            };
            if !allowed {
                continue;
            }
            let relative = PathBuf::from("crash-reports").join(directory).join(name);
            if directory == "panics"
                && stem
                    .parse::<u32>()
                    .ok()
                    .filter(|pid| *pid != 0)
                    .is_none_or(|pid| {
                        pid == std::process::id() || super::platform::process_is_alive(pid, "")
                    })
            {
                entries
                    .push(json!({"source":relative,"status":"active_or_unverified_owner_omitted"}));
                continue;
            }
            if directory == "sessions" {
                match metadata_json(&entry.path()) {
                    Ok((marker, hash)) => {
                        metadata_hashes.insert(relative.clone(), hash.clone());
                        let pid = marker["pid"]
                            .as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .filter(|pid| *pid != 0);
                        let host = marker["host_process"]
                            .as_str()
                            .filter(|host| !host.is_empty() && host.len() <= HOST_PROCESS_LIMIT);
                        if pid.zip(host).is_none_or(|(pid, host)| {
                            pid == std::process::id()
                                || super::platform::process_is_alive(pid, host)
                        }) {
                            entries.push(json!({"source":relative,"status":"active_or_unverified_owner_omitted","reason":if marker["host_process"].as_str().is_some_and(|host|host.len()>HOST_PROCESS_LIMIT){"owner_process_identifier_exceeds_4096_byte_limit"}else{"owner_active_or_unverified"}}));
                            continue;
                        }
                        if let Some((pid, host)) = pid.zip(host) {
                            owners.insert(
                                relative.clone(),
                                (pid, host.into(), entry.path(), hash.clone()),
                            );
                        }
                        if let Some(journal) = marker["journal_file"].as_str() {
                            if session_journal(journal) {
                                let journal = PathBuf::from("crash-reports/sessions").join(journal);
                                if let Some((pid, host)) = pid.zip(host) {
                                    owners.insert(
                                        journal.clone(),
                                        (pid, host.into(), entry.path(), hash.clone()),
                                    );
                                }
                                sources.insert(journal);
                            } else {
                                warnings.push(
                                    "Session journal reference outside owned directory omitted"
                                        .into(),
                                );
                            }
                        }
                    }
                    Err(e) => {
                        entries.push(json!({"source":relative,"status":"metadata_unavailable","reason":e.to_string()}));
                        warnings.push("Session ownership could not be verified".into());
                        continue;
                    }
                }
            }
            sources.insert(relative);
        }
    }
    // Saved references make missing originals visible instead of silently
    // reporting complete coverage after a source was moved or disappeared.
    let metadata: Vec<_> = sources
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .cloned()
        .collect();
    for relative in metadata {
        let metadata_source = cache.join(&relative);
        if let Ok((value, hash)) = metadata_json(&metadata_source) {
            if metadata_hashes
                .get(&relative)
                .is_some_and(|previous| *previous != hash)
            {
                warnings.push(format!(
                    "Evidence ownership metadata changed during selection: {}",
                    relative.display()
                ));
            } else {
                metadata_hashes.insert(relative.clone(), hash.clone());
            }
            if let Some(file) = value["local_journal"]["local_file"]
                .as_str()
                .or(value["local_file"].as_str())
            {
                let path = Path::new(file);
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let (stem, extension) = filename.rsplit_once('.').unwrap_or((filename, ""));
                let owned_name = if file.starts_with("originals/") {
                    (extension == "dfr" && hex(stem, 16)) || (extension == "raw" && hex(stem, 64))
                } else if file.starts_with("sessions/") {
                    session_journal(filename)
                } else {
                    false
                };
                if owned_name
                    && path
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)))
                    && path.components().count() == 2
                {
                    let recorded = if value["local_journal"].is_object() {
                        &value["local_journal"]
                    } else {
                        &value
                    };
                    let recorded = json!({"metadata_source":relative,
                        "bytes":recorded["bytes"].as_u64(),
                        "blake3":recorded["blake3"].as_str().filter(|hash| hex(hash, 64)),
                        "omitted_slots":recorded["omitted_slots"].as_u64(),
                        "capture_error_present":!recorded["capture_error"].is_null()});
                    let relative = PathBuf::from("crash-reports").join(path);
                    if references.len() < FILE_LIMIT {
                        references.entry(relative.clone()).or_insert(recorded);
                    }
                    if file.starts_with("sessions/") {
                        let owner = value["pid"]
                            .as_u64()
                            .and_then(|n| u32::try_from(n).ok())
                            .filter(|pid| *pid != 0)
                            .zip(value["host_process"].as_str().filter(|host| {
                                !host.is_empty() && host.len() <= HOST_PROCESS_LIMIT
                            }));
                        if owner.is_none_or(|(pid, host)| {
                            pid == std::process::id()
                                || super::platform::process_is_alive(pid, host)
                        }) {
                            entries.push(json!({"source":relative,"status":"active_or_unverified_owner_omitted","reason":if value["host_process"].as_str().is_some_and(|host|host.len()>HOST_PROCESS_LIMIT){"owner_process_identifier_exceeds_4096_byte_limit"}else{"owner_active_or_unverified"}}));
                            continue;
                        }
                        if let Some((pid, host)) = owner {
                            owners.insert(
                                relative.clone(),
                                (pid, host.into(), metadata_source.clone(), hash.clone()),
                            );
                        }
                    }
                    if sources.len() < FILE_LIMIT {
                        sources.insert(relative);
                    } else {
                        warnings.push("Crash evidence reference limit reached".into());
                        entries.push(json!({"source":relative,"status":"selection_limit_omitted"}));
                    }
                } else {
                    warnings.push("Unsafe saved evidence reference omitted".into());
                }
            }
        } else {
            warnings.push(format!(
                "Saved evidence reference metadata unavailable: {}",
                relative.display()
            ));
        }
    }
    if sources.len() > FILE_LIMIT {
        warnings.push("Crash evidence file limit reached".into());
    }
    for relative in session_journals {
        if !sources.contains(&relative) {
            entries.push(json!({"source":relative,"status":"active_or_unverified_owner_omitted"}));
        }
    }
    for (index, relative) in sources.into_iter().enumerate() {
        if index >= FILE_LIMIT {
            entries.push(json!({"source":relative,"status":"selection_limit_omitted"}));
            continue;
        }
        stopped(stopping)?;
        if owners.get(&relative).is_some_and(|(pid, host, _, _)| {
            *pid == std::process::id() || super::platform::process_is_alive(*pid, host)
        }) {
            entries.push(json!({"source":relative,"status":"active_owner_omitted"}));
            continue;
        }
        let source = cache.join(&relative);
        let output = target.join(&relative);
        let result = copy_owned(&source, &output, stopping, owners.get(&relative));
        match result {
            Ok((bytes, digest)) => {
                let changed = metadata_hashes
                    .get(&relative)
                    .is_some_and(|selected| *selected != digest);
                if changed {
                    warnings.push(format!("Evidence metadata changed between selection and capture; original-reference coverage may be incomplete: {}",relative.display()));
                }
                entries.push(json!({"source":relative,"export_file":PathBuf::from("crash-evidence").join(&relative),"status":"complete","bytes":bytes,"blake3":digest,"metadata_changed_since_selection":changed,"unredacted_private_original":true}));
            }
            Err(e) => {
                let status = if e.kind() == std::io::ErrorKind::NotFound {
                    "missing"
                } else if e.kind() == std::io::ErrorKind::WouldBlock {
                    "publisher_active_omitted"
                } else {
                    "incomplete"
                };
                entries.push(json!({"source":relative,"status":status,"export_file":if output.exists(){Some(PathBuf::from("crash-evidence").join(&relative))}else{None},"reason":e.to_string()}));
                warnings.push(format!(
                    "Private crash evidence {status}: {}",
                    relative.display()
                ));
                if e.kind() == std::io::ErrorKind::Interrupted {
                    return Err(e);
                }
            }
        }
    }
    for entry in &mut entries {
        if let Some(recorded) = entry["source"]
            .as_str()
            .and_then(|path| references.get(Path::new(path)))
        {
            if entry["status"] == "complete"
                && entry["source"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(".dfr"))
                && (recorded["bytes"]
                    .as_u64()
                    .is_some_and(|bytes| entry["bytes"].as_u64() != Some(bytes))
                    || recorded["blake3"]
                        .as_str()
                        .is_some_and(|hash| entry["blake3"].as_str() != Some(hash)))
            {
                entry["status"] = json!("content_identity_mismatch");
                warnings.push(format!("Copied crash journal differs from its recorded original length or content hash: {}",entry["source"].as_str().unwrap_or("unknown")));
            }
            entry["recorded_original"] = recorded.clone();
        }
    }
    let partial = !warnings.is_empty() || entries.iter().any(|e| e["status"] != "complete");
    let manifest = json!({"schema":1,"scope":"user_requested_local_export_only","automatically_uploaded":false,"paths_redacted":false,"privacy":"Exact private originals may contain personal paths or sensitive native fields. Review before sharing; the structured log redaction setting does not alter these copies.","coverage":if partial{"partial"}else{"complete_at_capture"},"file_limit":FILE_LIMIT,"selection":"Known artifacts already archived in the private crash cache; live session owners and publisher jobs are excluded", "os_report_directories_searched":false,"binary_process_dumps_included":false,"metadata_read_limit_bytes":METADATA_LIMIT,"owner_process_identifier_limit_bytes":HOST_PROCESS_LIMIT,"entries":entries,"warnings":warnings});
    let mut file = private_new(&target.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.sync_all()?;
    Ok(CrashExport { manifest, warnings })
}

fn copy_owned(
    source: &Path,
    target: &Path,
    stopping: &AtomicBool,
    owner: Option<&(u32, String, PathBuf, String)>,
) -> std::io::Result<(u64, String)> {
    let _guard = source_lock(source)?;
    let mut owner_guard = None;
    if let Some((pid, host, marker, selected_hash)) = owner {
        if marker != source {
            owner_guard = Some(source_lock(marker)?);
        }
        let (_, current_hash) = metadata_json(marker)?;
        if current_hash != *selected_hash
            || *pid == std::process::id()
            || super::platform::process_is_alive(*pid, host)
        {
            return Err(std::io::Error::other(
                "Session ownership changed or became active before capture",
            ));
        }
    }
    let _owner_guard = owner_guard;
    let mut input = File::open(source)?;
    let before = input.metadata()?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut output = private_new(target)?;
    let result = (|| {
        let mut remaining = before.len();
        let mut buffer = [0_u8; 64 * 1024];
        let mut hash = blake3::Hasher::new();
        while remaining != 0 {
            stopped(stopping)?;
            let count = input.read(&mut buffer[..(remaining.min(64 * 1024)) as usize])?;
            if count == 0 {
                return Err(std::io::Error::other(
                    "Evidence changed or ended during export",
                ));
            }
            output.write_all(&buffer[..count])?;
            hash.update(&buffer[..count]);
            remaining -= count as u64;
        }
        stopped(stopping)?;
        let after = input.metadata()?;
        let mut extra = [0_u8; 1];
        if input.read(&mut extra)? != 0
            || after.len() != before.len()
            || after.modified().ok() != before.modified().ok()
        {
            return Err(std::io::Error::other("Evidence changed during export"));
        }
        let digest = hash.finalize().to_hex().to_string();
        if source.extension().is_some_and(|e| e == "raw")
            && source
                .file_stem()
                .and_then(|n| n.to_str())
                .is_some_and(|n| hex(n, 64) && n != digest)
        {
            return Err(std::io::Error::other(
                "Native original differs from its recorded content identity",
            ));
        }
        output.sync_all()?;
        Ok((before.len(), digest))
    })();
    drop(output);
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

fn source_lock(source: &Path) -> std::io::Result<File> {
    if !plain(source, false) {
        return Err(std::fs::symlink_metadata(source)
            .err()
            .unwrap_or_else(|| std::io::Error::other("Symlink or non-file evidence omitted")));
    }
    let parent = source
        .parent()
        .ok_or_else(|| std::io::Error::other("Missing source owner"))?;
    if !plain(parent, true) {
        return Err(std::io::Error::other(
            "Evidence parent is not an owned directory",
        ));
    }
    let lock_path = buffr_durable_file::lock_path(source)
        .ok_or_else(|| std::io::Error::other("Invalid evidence identity"))?;
    if std::fs::symlink_metadata(&lock_path).is_ok() && !plain(&lock_path, false) {
        return Err(std::io::Error::other("Unsafe evidence lock"));
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let guard = options.open(lock_path)?;
    guard.try_lock().map_err(|e| match e {
        std::fs::TryLockError::WouldBlock => std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "Evidence publisher is active",
        ),
        std::fs::TryLockError::Error(e) => e,
    })?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_owner_replacement_between_selection_and_capture_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("dead.json");
        let source = directory.path().join("dead.dfr");
        let target = directory.path().join("export.dfr");
        let original = b"complete authored retained crash journal";
        std::fs::write(&source, original).unwrap();
        let old = serde_json::to_vec(
            &json!({"pid":u32::MAX,"host_process":"authored-dead-host","journal_file":"dead.dfr"}),
        )
        .unwrap();
        buffr_durable_file::publish_private(&marker, &old).unwrap();
        let (_, selected_hash) = metadata_json(&marker).unwrap();
        let owner = (
            u32::MAX,
            "authored-dead-host".into(),
            marker.clone(),
            selected_hash,
        );
        let publisher = source_lock(&marker).unwrap();
        assert_eq!(
            copy_owned(&source, &target, &AtomicBool::new(false), Some(&owner))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert!(!target.exists());
        drop(publisher);
        let replacement = serde_json::to_vec(&json!({"pid":std::process::id(),"host_process":"current-host","journal_file":"dead.dfr"})).unwrap();
        buffr_durable_file::publish_private(&marker, &replacement).unwrap();
        assert!(
            copy_owned(&source, &target, &AtomicBool::new(false), Some(&owner)).is_err(),
            "a stale dead-owner observation cannot authorize a fresh active source"
        );
        assert!(!target.exists());
        assert_eq!(std::fs::read(&source).unwrap(), original);
        assert_eq!(std::fs::read(&marker).unwrap(), replacement);
        buffr_durable_file::publish_private(&marker, &old).unwrap();
        copy_owned(&source, &target, &AtomicBool::new(false), Some(&owner)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), original);
    }
}
