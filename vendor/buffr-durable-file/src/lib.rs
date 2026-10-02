use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Every artifact this crate creates beside a published file is a dotfile.
const ARTIFACT_PREFIX: char = '.';
/// Suffix of the advisory lock published beside a file.
const LOCK_SUFFIX: &str = ".lock";
/// Separator between a published file name and the `{pid}-{sequence}` suffix of
/// its in-flight temporary.
const TEMPORARY_INFIX: &str = ".tmp-";

/// Path of the advisory lock `publish` takes beside `path`.
///
/// Returns `None` when `path` has no parent directory or no UTF-8 file name,
/// exactly the cases in which publication itself fails.
#[must_use]
pub fn lock_path(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let file_name = path.file_name()?.to_str()?;
    Some(parent.join(lock_file_name(file_name)))
}

fn lock_file_name(file_name: &str) -> String {
    format!("{ARTIFACT_PREFIX}{file_name}{LOCK_SUFFIX}")
}

fn temporary_file_name(file_name: &str, process: u32, sequence: u64) -> String {
    format!("{ARTIFACT_PREFIX}{file_name}{TEMPORARY_INFIX}{process}-{sequence}")
}

/// Whether `file_name` is an advisory lock created by this crate.
///
/// A lock may be held by a live process, so it must only be deleted once the
/// owning session is known to be gone.
#[must_use]
pub fn is_lock_file_name(file_name: &str) -> bool {
    file_name
        .strip_prefix(ARTIFACT_PREFIX)
        .and_then(|name| name.strip_suffix(LOCK_SUFFIX))
        .is_some_and(|target| !target.is_empty())
}

/// Whether `file_name` is an in-flight temporary created by this crate.
///
/// A temporary is only ever left behind by a process that died mid-publish, so
/// it never carries data another session can still reach.
#[must_use]
pub fn is_temporary_file_name(file_name: &str) -> bool {
    let Some(name) = file_name.strip_prefix(ARTIFACT_PREFIX) else {
        return false;
    };
    let Some((target, suffix)) = name.rsplit_once(TEMPORARY_INFIX) else {
        return false;
    };
    let Some((process, sequence)) = suffix.split_once('-') else {
        return false;
    };
    !target.is_empty() && process.parse::<u32>().is_ok() && sequence.parse::<u64>().is_ok()
}

/// Whether `file_name` is any bookkeeping artifact created by this crate rather
/// than a published file.
#[must_use]
pub fn is_internal_file_name(file_name: &str) -> bool {
    is_lock_file_name(file_name) || is_temporary_file_name(file_name)
}

/// Atomically publishes `bytes` at `path`.
///
/// The bytes are written to a temporary beside `path`, synced, then renamed over
/// `path` while holding the advisory lock from [`lock_path`]. Readers that open
/// `path` see either the previous or the new contents, never a torn file.
///
/// # Windows
///
/// - The lock is a mandatory `LockFileEx` byte-range lock on the `.lock` dotfile.
///   Only publishers open that file; nothing should ever read it.
/// - The rename is `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`. Windows
///   refuses to replace a destination another process holds open (a reader in
///   a second BUFFR instance, an indexer, an antivirus scan), so transient
///   `ERROR_ACCESS_DENIED` / `ERROR_SHARING_VIOLATION` results are retried
///   briefly and then handed to `std::fs::rename`, whose POSIX-semantics rename
///   replaces a destination whose readers opened it with `FILE_SHARE_DELETE`
///   (every Rust `std` reader does).
/// - Deleting a lock while a live process holds it succeeds but leaves the name
///   delete-pending; a publisher opening that lock then fails with
///   [`std::io::ErrorKind::PermissionDenied`] until the holder closes it. That
///   is why [`is_lock_file_name`] callers must only reclaim locks of sessions
///   known to be gone. [`is_transient_sharing_error`] identifies the retryable
///   Windows outcomes for callers that want their own back-off.
///
/// Errors keep the originating [`std::io::ErrorKind`] and name the failed step
/// and path in their message.
pub fn publish(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    publish_with(path, bytes, false, replace_path)
}

/// Atomically publishes bytes, giving up if another publisher keeps the
/// advisory lock beyond `wait`. Intended for workers that must join on unload.
pub fn publish_with_lock_timeout(
    path: &Path,
    bytes: &[u8],
    wait: std::time::Duration,
) -> std::io::Result<()> {
    publish_streaming_with_lock(path, false, replace_path, Some(wait), |file| {
        file.write_all(bytes)
    })
}

/// Whether `error` is a Windows sharing conflict that a short retry can clear:
/// `ERROR_ACCESS_DENIED` (5), `ERROR_SHARING_VIOLATION` (32) or
/// `ERROR_LOCK_VIOLATION` (33). Always `false` on other platforms, where these
/// raw codes mean something else.
#[must_use]
pub fn is_transient_sharing_error(error: &std::io::Error) -> bool {
    if cfg!(windows) {
        matches!(error.raw_os_error(), Some(5 | 32 | 33))
    } else {
        false
    }
}

pub fn publish_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    publish_with(path, bytes, true, replace_path)
}

pub fn publish_streaming(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    publish_streaming_with(path, false, replace_path, write)
}

fn publish_with(
    path: &Path,
    bytes: &[u8],
    private: bool,
    replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    publish_streaming_with(path, private, replace, |file| file.write_all(bytes))
}

fn publish_streaming_with(
    path: &Path,
    private: bool,
    replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    publish_streaming_with_lock(path, private, replace, None, write)
}

fn publish_streaming_with_lock(
    path: &Path,
    private: bool,
    replace: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    lock_wait: Option<std::time::Duration>,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use fs4::FileExt;

    #[cfg(not(unix))]
    let _ = private;

    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("publication path has no parent"))?;
    std::fs::create_dir_all(parent).map_err(|error| at_path("create", parent, error))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::other("publication path has an invalid filename"))?;
    let lock_path = parent.join(lock_file_name(file_name));
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| at_path("open", &lock_path, error))?;
    if let Some(wait) = lock_wait {
        let deadline = std::time::Instant::now() + wait;
        loop {
            match lock_file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => return Err(at_path("lock", &lock_path, error.into())),
            }
        }
    } else {
        FileExt::lock(&lock_file).map_err(|error| at_path("lock", &lock_path, error))?;
    }

    let (temporary, mut temp_file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(temporary_file_name(file_name, std::process::id(), sequence));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&temporary) {
            Ok(file) => break (temporary, file),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(at_path("create", &temporary, error)),
        }
    };

    // Removes the temporary on every exit but a successful replace, including
    // a panic in `write`.
    struct RemoveOnDrop<'a>(Option<&'a Path>);
    impl Drop for RemoveOnDrop<'_> {
        fn drop(&mut self) {
            if let Some(path) = self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let mut cleanup = RemoveOnDrop(Some(&temporary));

    write(&mut temp_file).map_err(|error| at_path("write", &temporary, error))?;
    temp_file
        .sync_all()
        .map_err(|error| at_path("sync", &temporary, error))?;
    drop(temp_file);
    replace(&temporary, path).map_err(|error| at_path("replace", path, error))?;
    cleanup.0 = None;
    Ok(())
}

fn at_path(action: &str, path: &Path, source: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        source.kind(),
        format!("could not {action} {}: {source}", path.display()),
    )
}

#[cfg(not(windows))]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    replace_path_with(
        source,
        destination,
        |source, destination| std::fs::rename(source, destination),
        |parent| std::fs::File::open(parent)?.sync_all(),
    )
}

#[cfg(not(windows))]
fn replace_path_with(
    source: &Path,
    destination: &Path,
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    sync_parent: impl FnOnce(&Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    rename(source, destination)?;
    let parent = destination.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "replacement destination has no parent directory",
        )
    })?;
    sync_parent(parent)
}

/// Retry schedule for transient Windows sharing conflicts: about 60 ms total.
const TRANSIENT_RETRY_DELAYS: [std::time::Duration; 6] = [
    std::time::Duration::from_millis(1),
    std::time::Duration::from_millis(2),
    std::time::Duration::from_millis(4),
    std::time::Duration::from_millis(8),
    std::time::Duration::from_millis(16),
    std::time::Duration::from_millis(32),
];

/// Windows replacement contract: a write-through `MoveFileEx`, retried through
/// `sleep` while `is_transient` accepts the error, then one POSIX-semantics
/// rename as the last resort for a destination that stays open elsewhere.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "Windows contract kept portable so Linux CI can test it"
    )
)]
fn replace_path_windows_with(
    mut write_through: impl FnMut() -> std::io::Result<()>,
    posix_replace: impl FnOnce() -> std::io::Result<()>,
    is_transient: impl Fn(&std::io::Error) -> bool,
    mut sleep: impl FnMut(std::time::Duration),
) -> std::io::Result<()> {
    let mut delays = TRANSIENT_RETRY_DELAYS.iter();
    loop {
        let error = match write_through() {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if !is_transient(&error) {
            return Err(error);
        }
        match delays.next() {
            Some(delay) => sleep(*delay),
            None => return posix_replace().map_err(|_| error),
        }
    }
}

/// Adds the `\\?\` prefix a fully qualified, normalized Windows path needs to
/// exceed `MAX_PATH`, mirroring what `std` does for its own file APIs.
///
/// `absolute` is a NUL-terminated UTF-16 path that `GetFullPathNameW` already
/// normalized (backslashes, no `.`/`..`). Short paths and paths that already
/// carry a `\\?\` or `\??\` prefix are returned unchanged; relative paths are
/// left alone because a verbatim prefix would break them.
#[cfg_attr(
    not(windows),
    allow(
        dead_code,
        reason = "Windows contract kept portable so Linux CI can test it"
    )
)]
fn windows_long_path(absolute: &[u16]) -> Vec<u16> {
    const LEGACY_MAX_PATH: usize = 248;
    const SEP: u16 = b'\\' as u16;
    const QUERY: u16 = b'?' as u16;
    const COLON: u16 = b':' as u16;
    const DOT: u16 = b'.' as u16;
    const VERBATIM: &[u16] = &[SEP, SEP, QUERY, SEP];
    const NT: &[u16] = &[SEP, QUERY, QUERY, SEP];
    const UNC: &[u16] = &[
        SEP,
        SEP,
        QUERY,
        SEP,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        SEP,
    ];

    if absolute.len() < LEGACY_MAX_PATH
        || absolute.starts_with(VERBATIM)
        || absolute.starts_with(NT)
    {
        return absolute.to_vec();
    }
    let (prefix, rest): (&[u16], &[u16]) = match absolute {
        [_, COLON, SEP, ..] => (VERBATIM, absolute),
        [SEP, SEP, DOT, SEP, ..] => (VERBATIM, &absolute[4..]),
        [SEP, SEP, ..] => (UNC, &absolute[2..]),
        _ => (&[], absolute),
    };
    let mut path = Vec::with_capacity(prefix.len() + rest.len());
    path.extend_from_slice(prefix);
    path.extend_from_slice(rest);
    path
}

#[cfg(windows)]
fn windows_wide_path(path: &Path) -> std::io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetFullPathNameW;

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path contains an interior NUL",
        ));
    }
    if wide.len() < 248 {
        return Ok(wide);
    }
    // SAFETY: `wide` is NUL-terminated and outlives both calls; a null buffer with
    // zero length asks only for the required size.
    let required =
        unsafe { GetFullPathNameW(wide.as_ptr(), 0, std::ptr::null_mut(), std::ptr::null_mut()) };
    if required == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut absolute = vec![0_u16; required as usize];
    // SAFETY: `absolute` has `required` writable code units, which includes the NUL.
    let written = unsafe {
        GetFullPathNameW(
            wide.as_ptr(),
            required,
            absolute.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    if written == 0 || written as usize >= absolute.len() {
        return Err(std::io::Error::last_os_error());
    }
    absolute.truncate(written as usize + 1);
    Ok(windows_long_path(&absolute))
}

#[cfg(windows)]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source_wide = windows_wide_path(source)?;
    let destination_wide = windows_wide_path(destination)?;
    replace_path_windows_with(
        || {
            // SAFETY: Both paths are owned, NUL-terminated UTF-16 buffers that remain
            // alive for the call.
            let moved = unsafe {
                MoveFileExW(
                    source_wide.as_ptr(),
                    destination_wide.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            };
            if moved == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        },
        || std::fs::rename(source, destination),
        is_transient_sharing_error,
        std::thread::sleep,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    const PROCESS_PATH_ENV: &str = "BUFFR_ATOMIC_JSON_PROCESS_PATH";

    /// A fresh directory under `TMPDIR`, removed on drop.
    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "buffr-durable-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn names(&self) -> Vec<String> {
            std::fs::read_dir(&self.0)
                .expect("read test directory")
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        }

        fn assert_no_temporaries(&self) {
            let names = self.names();
            assert!(
                !names.iter().any(|name| name.contains(".tmp-")),
                "{names:?}"
            );
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write_many(path: &Path, writer: u32) {
        for sequence in 0..100 {
            let bytes = format!(
                r#"{{"writer":{writer},"sequence":{sequence},"payload":"{}"}}"#,
                "x".repeat(512)
            );
            publish(path, bytes.as_bytes()).expect("publish test JSON");
        }
    }

    /// Readers never see torn JSON while threads and separate processes publish
    /// concurrently, and no temporary survives.
    #[test]
    fn concurrent_threads_and_processes_never_publish_partial_json() {
        let dir = Dir::new();
        let path = dir.0.join("route.json");
        publish(&path, br#"{"writer":0}"#).expect("write initial JSON");
        let helper = format!("{}::process_writer_helper", module_path!());
        let helper = helper
            .split_once("::")
            .map_or(helper.clone(), |(_, name)| name.to_owned());

        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (path, done) = (path.clone(), Arc::clone(&done));
            std::thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    let text = std::fs::read_to_string(&path).expect("published JSON exists");
                    serde_json::from_str::<serde_json::Value>(&text).expect("JSON is complete");
                }
            })
        };
        let children = (1..=2)
            .map(|_| {
                std::process::Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", &helper, "--nocapture", "--test-threads=1"])
                    .env(PROCESS_PATH_ENV, &path)
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .expect("spawn process writer")
            })
            .collect::<Vec<_>>();
        let threads = (3..=4)
            .map(|writer| {
                let path = path.clone();
                std::thread::spawn(move || write_many(&path, writer))
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().expect("writer joined");
        }
        for child in children {
            let output = child.wait_with_output().expect("wait for process writer");
            assert!(output.status.success());
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("1 passed"), "helper must run: {stdout}");
        }
        done.store(true, Ordering::Release);
        reader.join().expect("reader joined");

        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).expect("read"))
            .expect("final JSON is valid");
        dir.assert_no_temporaries();
    }

    #[test]
    fn process_writer_helper() {
        if let Some(path) = std::env::var_os(PROCESS_PATH_ENV) {
            write_many(Path::new(&path), std::process::id());
        }
    }

    /// A failing replace, a failing writer or a panicking writer keeps the prior
    /// file byte-identical and removes the temporary.
    #[test]
    fn failed_publication_preserves_prior_file_and_cleans_temporary() {
        let dir = Dir::new();
        let path = dir.0.join("route.json");
        std::fs::write(&path, b"prior").expect("write prior file");

        let error = publish_with(&path, b"next", false, |_, _| {
            Err(std::io::Error::other("injected replacement failure"))
        })
        .expect_err("replacement fails");
        assert!(error.to_string().contains("injected replacement failure"));
        let error = publish_streaming(&path, |_| {
            Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
        })
        .expect_err("write fails");
        assert_eq!(error.kind(), std::io::ErrorKind::StorageFull);
        let panicked = std::panic::catch_unwind(|| {
            let _ = publish_streaming(&path, |_| panic!("injected writer panic"));
        });
        assert!(panicked.is_err());

        assert_eq!(std::fs::read(&path).expect("read prior file"), b"prior");
        dir.assert_no_temporaries();
    }

    /// Publishing over a destination a reader holds open succeeds, and leaves
    /// only the published file plus a lock that `is_lock_file_name` recognises.
    #[test]
    fn publication_replaces_an_open_destination_and_leaves_a_recognised_lock() {
        let dir = Dir::new();
        let path = dir.0.join("route.json");
        publish(&path, b"prior").expect("write prior file");
        let reader = std::fs::File::open(&path).expect("open destination");
        publish(&path, b"next").expect("publish over an open destination");
        drop(reader);

        assert_eq!(std::fs::read(&path).expect("read"), b"next");
        let lock = lock_path(&path).expect("lock path");
        assert!(lock.is_file(), "publication must leave its advisory lock");
        assert!(is_lock_file_name(
            &lock.file_name().unwrap().to_string_lossy()
        ));
        let published: Vec<_> = dir
            .names()
            .into_iter()
            .filter(|name| !is_internal_file_name(name))
            .collect();
        assert_eq!(published, ["route.json"]);
    }

    #[test]
    fn bounded_publish_returns_when_inner_lock_is_held() {
        use fs4::FileExt;

        let dir = Dir::new();
        let path = dir.0.join("preferences.txt");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path(&path).expect("lock path"))
            .expect("open inner lock");
        FileExt::lock(&lock).expect("hold inner lock");
        let result = publish_with_lock_timeout(&path, b"new", std::time::Duration::from_millis(25));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        assert!(!path.exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn posix_replacement_syncs_parent_after_rename() {
        let events = std::cell::RefCell::new(Vec::new());
        replace_path_with(
            Path::new("/d/route.json.tmp"),
            Path::new("/d/route.json"),
            |source, destination| {
                assert_eq!(
                    (source, destination),
                    (Path::new("/d/route.json.tmp"), Path::new("/d/route.json"))
                );
                events.borrow_mut().push("rename");
                Ok(())
            },
            |parent| {
                assert_eq!(parent, Path::new("/d"));
                events.borrow_mut().push("sync-parent");
                Ok(())
            },
        )
        .expect("replacement contract succeeds");
        assert_eq!(*events.borrow(), ["rename", "sync-parent"]);
    }

    /// The Windows replace retries only transient errors, sleeping through
    /// `TRANSIENT_RETRY_DELAYS` in order (under 100 ms total), then falls back to a
    /// POSIX rename once, reporting the original error if that fails too.
    #[test]
    fn windows_replacement_matches_the_retry_model() {
        let sharing_violation = || std::io::Error::from_raw_os_error(32);
        let retries = TRANSIENT_RETRY_DELAYS.len();
        for failures in 0..=retries + 2 {
            for transient in [true, false] {
                for fallback_ok in [true, false] {
                    let attempts = std::cell::Cell::new(0);
                    let fallback = std::cell::Cell::new(false);
                    let slept = std::cell::RefCell::new(Vec::new());
                    let result = replace_path_windows_with(
                        || {
                            attempts.set(attempts.get() + 1);
                            match attempts.get() > failures {
                                true => Ok(()),
                                false if transient => Err(sharing_violation()),
                                false => Err(std::io::ErrorKind::NotFound.into()),
                            }
                        },
                        || {
                            fallback.set(true);
                            fallback_ok
                                .then_some(())
                                .ok_or_else(|| std::io::Error::other("x"))
                        },
                        |error| error.raw_os_error() == Some(32),
                        |delay| slept.borrow_mut().push(delay),
                    );
                    let case = format!("{failures} {transient} {fallback_ok}");
                    let (want_attempts, want_sleeps, want_fallback, want_ok) = if failures == 0 {
                        (1, 0, false, true)
                    } else if !transient {
                        (1, 0, false, false)
                    } else if failures <= retries {
                        (failures + 1, failures, false, true)
                    } else {
                        (retries + 1, retries, true, fallback_ok)
                    };
                    assert_eq!(attempts.get(), want_attempts, "{case}");
                    assert_eq!(
                        &*slept.borrow(),
                        &TRANSIENT_RETRY_DELAYS[..want_sleeps],
                        "{case}"
                    );
                    assert_eq!(fallback.get(), want_fallback, "{case}");
                    match result {
                        Ok(()) => assert!(want_ok, "{case}"),
                        Err(error) if transient => {
                            assert_eq!(error.raw_os_error(), Some(32), "{case}")
                        }
                        Err(error) => {
                            assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{case}")
                        }
                    }
                }
            }
        }
        let total: std::time::Duration = TRANSIENT_RETRY_DELAYS.iter().sum();
        assert!(total < std::time::Duration::from_millis(100));
        for (code, windows_only) in [(5, true), (32, true), (33, true), (2, false)] {
            let error = std::io::Error::from_raw_os_error(code);
            assert_eq!(
                is_transient_sharing_error(&error),
                windows_only && cfg!(windows)
            );
        }
        assert!(!is_transient_sharing_error(&std::io::Error::other("x")));
    }

    /// Every lock and temporary name publication can create is recognised as
    /// internal; no published name or near-miss is (a real-name misread would
    /// delete user data during recovery).
    #[test]
    fn artifact_predicates_match_exactly_the_names_publication_creates() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        const ALPHABET: &[u8] = b"ab9.-_tmplock";
        for _ in 0..2_000 {
            let len = 1 + (next() % 16) as usize;
            let name: String = (0..len)
                .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize] as char)
                .collect();
            let name = name.trim_start_matches('.');
            if name.is_empty() {
                continue;
            }
            assert!(!is_internal_file_name(name), "{name}");
            assert!(is_lock_file_name(&lock_file_name(name)), "{name}");
            let temporary = temporary_file_name(name, next() as u32, next());
            assert!(is_temporary_file_name(&temporary), "{temporary}");
        }
        for name in [
            ".lock",
            "..lock",
            ".spill.bin.tmp",
            ".spill.bin.tmp-",
            ".spill.bin.tmp-abc-1",
            ".spill.bin.tmp-1",
            ".spill.bin.tmp-1-x",
            ".tmp-1-2",
            "spill.bin.lock",
        ] {
            assert!(!is_internal_file_name(name), "{name}");
        }
    }

    #[test]
    fn windows_long_paths_gain_a_verbatim_prefix_only_when_needed() {
        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<_>>();
        let deep = "\\".to_owned() + &"BUFFR Exports \u{e9}\\".repeat(20);
        let tail = "x".repeat(300);
        for (input, expected) in [
            (format!("C:{deep}r.json"), format!("\\\\?\\C:{deep}r.json")),
            (
                format!("\\\\server\\share{deep}r.json"),
                format!("\\\\?\\UNC\\server\\share{deep}r.json"),
            ),
            (
                format!("\\\\.\\C:{deep}r.json"),
                format!("\\\\?\\C:{deep}r.json"),
            ),
            (String::new(), String::new()),
            (
                "C:\\Users\\Zo\u{eb}\\AppData\\BUFFR\\p.txt".into(),
                "C:\\Users\\Zo\u{eb}\\AppData\\BUFFR\\p.txt".into(),
            ),
            (
                "\\\\server\\share\\x.json".into(),
                "\\\\server\\share\\x.json".into(),
            ),
            (format!("\\\\?\\C:\\{tail}"), format!("\\\\?\\C:\\{tail}")),
            (format!("\\??\\C:\\{tail}"), format!("\\??\\C:\\{tail}")),
            (format!("relative\\{tail}"), format!("relative\\{tail}")),
        ] {
            assert_eq!(windows_long_path(&wide(&input)), wide(&expected), "{input}");
        }
    }
}
