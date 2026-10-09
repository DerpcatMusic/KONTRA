use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use ureq::unversioned::{
    resolver::{ResolvedSocketAddrs, Resolver},
    transport::{DefaultConnector, NextTimeout},
};

const ENDPOINT: &str = "https://matari-audio.com/api/support/mui";
const REPORT_BYTES: usize = 64 * 1024;
const PENDING: usize = 32;
static ACTIVE: Mutex<Weak<Worker>> = Mutex::new(Weak::new());

#[derive(Debug)]
struct JoinedResolver(
    hickory_resolver::config::ResolverConfig,
    hickory_resolver::config::ResolverOpts,
);

impl Resolver for JoinedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        use std::net::{IpAddr, SocketAddr};
        let host = uri.host().ok_or(ureq::Error::HostNotFound)?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = uri
            .port_u16()
            .or_else(|| match uri.scheme_str() {
                Some("https") => Some(443),
                Some("http") => Some(80),
                _ => None,
            })
            .ok_or(ureq::Error::HostNotFound)?;
        let ips = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![ip]
        } else {
            // DNS tasks share this reporting thread and are cancelled before it joins.
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let deadline = (*timeout.after).min(Duration::from_secs(5));
            runtime.block_on(async {
                let resolver = hickory_resolver::Resolver::builder_with_config(
                    self.0.clone(),
                    hickory_resolver::net::runtime::TokioRuntimeProvider::default(),
                )
                .with_options(self.1.clone())
                .build()
                .map_err(io::Error::other)?;
                let lookup = tokio::time::timeout(deadline, resolver.lookup_ip(host))
                    .await
                    .map_err(|_| ureq::Error::Timeout(timeout.reason))?
                    .map_err(io::Error::other)?;
                Ok::<_, ureq::Error>(lookup.iter().collect::<Vec<_>>())
            })?
        };
        let mut addresses = self.empty();
        for address in config
            .ip_family()
            .keep_wanted(ips.into_iter().map(|ip| SocketAddr::new(ip, port)))
            .take(16)
        {
            addresses.push(address);
        }
        if addresses.is_empty() {
            Err(ureq::Error::HostNotFound)
        } else {
            Ok(addresses)
        }
    }
}

/// App identity for automatic MUI issue reporting. No GitHub token belongs in an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub app: String,
    pub version: String,
    /// Optional build revision; include it because MUI's package version alone
    /// does not distinguish git revisions.
    pub build: String,
    /// MUI git pin, since package versions do not distinguish repository revisions.
    pub mui_revision: String,
    /// Private pending queue, shared between launches of this app.
    pub directory: PathBuf,
}

impl Config {
    pub fn new(app: &str, version: &str) -> Self {
        Self {
            app: app.into(),
            version: version.into(),
            build: String::new(),
            mui_revision: option_env!("MUI_BUILD_REVISION")
                .unwrap_or("unknown")
                .into(),
            directory: super::directory().join(app).join("pending"),
        }
    }
}

/// Retain this on the app/plugin's owned state, before creating MUI windows.
/// Clones and repeated registrations of the same identity share one worker.
/// Drop all windows first, then the last reporter, off the audio thread.
/// Shutdown joins the worker. HTTP has a five-second deadline, but the OS DNS
/// resolver may take longer; there is no detached timeout/DNS helper thread.
/// No detached thread can outlive a plugin library being unloaded.
#[derive(Clone)]
pub struct Reporter(Arc<Worker>);

pub(super) fn active_reporter() -> Option<Reporter> {
    ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .upgrade()
        .map(Reporter)
}

struct Worker {
    config: Config,
    wake: mpsc::SyncSender<()>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Reporter {
    /// Enable automatic local queuing and background delivery to MUI support.
    /// Offline/failed submissions remain on disk and retry on the next wake/load.
    pub fn start(config: Config) -> io::Result<Self> {
        if !identity(&config.app, 64)
            || !identity(&config.version, 64)
            || (!config.build.is_empty() && !identity(&config.build, 64))
            || !identity(&config.mui_revision, 64)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "app, version and build must be short identifiers",
            ));
        }
        let mut active = ACTIVE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(worker) = active.upgrade() {
            if worker.config != config {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "this loaded MUI image already reports for another app/build",
                ));
            }
            return Ok(Self(worker));
        }
        fs::create_dir_all(&config.directory)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let (wake, receiver) = mpsc::sync_channel(1);
        let directory = config.directory.clone();
        let (dns, dns_options) =
            hickory_resolver::system_conf::read_system_conf().map_err(io::Error::other)?;
        let resolver = JoinedResolver(dns, dns_options);
        let worker = thread::Builder::new()
            .name("mui-reports".into())
            .spawn(move || {
                let config = ureq::Agent::config_builder()
                    .user_agent("matari-mui-report/0.4.0")
                    .timeout_global(Some(Duration::from_secs(5)))
                    .max_redirects(0)
                    .build();
                let agent = ureq::Agent::with_parts(config, DefaultConnector::default(), resolver);
                let mut retry_at = None;
                while !stopped.load(Ordering::Acquire) {
                    recover_operations(&directory);
                    if retry_at.is_none_or(|at| Instant::now() >= at) {
                        retry_at = (!deliver_pending(&directory, &stopped, |body| {
                            send(&agent, ENDPOINT, body)
                        }))
                        .then(|| Instant::now() + Duration::from_secs(60));
                    }
                    let wait = retry_at.map_or(Duration::from_secs(60), |at| {
                        at.saturating_duration_since(Instant::now())
                    });
                    let _ = receiver.recv_timeout(wait);
                }
            })?;
        let worker = Arc::new(Worker {
            config,
            wake,
            stop,
            thread: Mutex::new(Some(worker)),
        });
        *active = Arc::downgrade(&worker);
        Ok(Self(worker))
    }

    /// Directory containing pending reports, useful for an app's diagnostics UI.
    pub fn pending_directory(&self) -> &Path {
        &self.0.config.directory
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        if let Some(worker) = self
            .thread
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = worker.join();
        }
    }
}

fn identity(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._+- ".contains(&c))
        && value != "."
        && value != ".."
}

/// Diagnostic messages may include private paths or credentials. Keep the
/// original local journal; upload only this bounded, sanitized view.
fn redact(text: &str) -> String {
    let redacted = text
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "authorization",
                "bearer ",
                "token",
                "password",
                "secret",
                "license",
                "api_key",
                "api key",
            ]
            .iter()
            .any(|key| lower.contains(key))
            {
                return "[redacted sensitive diagnostic]".into();
            }
            line.split_whitespace()
                .map(|word| {
                    if word.contains('@')
                        || word.contains(":\\")
                        || word.contains(":/")
                        || word.contains('\\')
                        || word.contains('/')
                    {
                        "[redacted path or address]"
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    super::bounded(&redacted, 2048).to_owned()
}

pub(super) fn enqueue(
    component: &str,
    stage: &str,
    message: &str,
    history: &[String],
    adapter: Option<&wgpu::AdapterInfo>,
) {
    let worker = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .upgrade();
    let Some(worker) = worker else {
        return;
    };
    let report = report(&worker.config, component, stage, message, history, adapter);
    match persist(&worker.config.directory, &report) {
        Ok(()) => {
            let _ = worker.wake.try_send(());
        }
        Err(error) => eprintln!("MUI report could not be queued; local journal retained: {error}"),
    }
}

fn report(
    config: &Config,
    component: &str,
    stage: &str,
    message: &str,
    history: &[String],
    adapter: Option<&wgpu::AdapterInfo>,
) -> Value {
    let host = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .filter(|s| identity(s, 64))
        .unwrap_or_else(|| "unknown".into());
    let mut report = json!({
        "app": config.app,
        "app_version": config.version,
        "build": config.build,
        "mui_version": env!("CARGO_PKG_VERSION"),
        "mui_revision": config.mui_revision,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "host": host,
        "component": component,
        "stage": stage,
        "message": redact(message),
        "breadcrumbs": history.iter().map(|s| redact(s)).collect::<Vec<_>>(),
        "gpu": adapter.map(|a| json!({
            "backend": format!("{:?}", a.backend), "vendor": a.vendor, "device": a.device,
            "name": super::bounded(&redact(&a.name), 256), "driver": super::bounded(&redact(&a.driver), 256),
            "driver_info": super::bounded(&redact(&a.driver_info), 512),
        })),
    });
    // Keep the newest evidence when escaped driver messages fill the budget.
    while serde_json::to_vec(&report).is_ok_and(|bytes| bytes.len() > REPORT_BYTES) {
        let history = report["breadcrumbs"]
            .as_array_mut()
            .expect("report history is an array");
        if history.is_empty() {
            break;
        }
        history.remove(0);
    }
    report
}

pub(super) struct Operation {
    file: fs::File,
    path: PathBuf,
}

impl Operation {
    fn create(directory: &Path, report: &Value) -> io::Result<Self> {
        let _lease = queue_lease(directory)?;
        if fs::read_dir(directory)?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|e| e == "active"))
            .take(PENDING)
            .count()
            >= PENDING
        {
            return Err(io::Error::other(
                "32 interrupted or live GPU operations; export diagnostics",
            ));
        }
        let temporary = temporary_path(directory);
        let path = temporary.with_extension("active");
        let result = (|| {
            let mut file = super::private_file(&temporary)?;
            file.try_lock().map_err(io::Error::other)?;
            file.write_all(&[0])?;
            file.write_all(&serde_json::to_vec(report)?)?;
            file.flush()?;
            // Publish only after holding the lock and writing complete evidence.
            fs::rename(&temporary, &path)?;
            Ok(Self { file, path })
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        // Mark completion while still locked, before another process can recover
        // it. Rust unwind is handled by the caller, so is not a native crash.
        let _ = self
            .file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.write_all(&[1]));
        let _ = fs::remove_file(&self.path);
    }
}

pub(super) fn operation(
    component: &str,
    stage: &str,
    message: &str,
    adapter: Option<&wgpu::AdapterInfo>,
) -> Option<Operation> {
    let worker = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .upgrade()?;
    let history = super::journal()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .history
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let report = report(
        &worker.config,
        component,
        &format!("interrupted_{}", super::bounded(stage, 36)),
        &format!("Process ended during this MUI operation; cause unconfirmed: {message}"),
        &history,
        adapter,
    );
    match Operation::create(&worker.config.directory, &report) {
        Ok(operation) => Some(operation),
        Err(error) => {
            eprintln!("MUI operation marker failed; journal retained: {error}");
            None
        }
    }
}

fn recover_operations(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for path in entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "active"))
        .take(PENDING)
    {
        let result = (|| -> io::Result<()> {
            let mut file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
            match file.try_lock() {
                Ok(()) => (),
                Err(fs::TryLockError::WouldBlock) => return Ok(()),
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
            let mut completed = [0];
            file.read_exact(&mut completed)?;
            if completed == [0] {
                let mut bytes = Vec::new();
                Read::by_ref(&mut file)
                    .take((REPORT_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > REPORT_BYTES {
                    return Err(io::Error::other("invalid GPU operation marker"));
                }
                persist(directory, &serde_json::from_slice(&bytes)?)?;
            } else if completed != [1] {
                return Err(io::Error::other("invalid GPU operation marker state"));
            }
            fs::remove_file(path)
        })();
        if let Err(error) = result {
            eprintln!("MUI operation recovery retained evidence: {error}");
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn pending_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .take(PENDING + 1)
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn temporary_path(directory: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!("{}-{stamp}.tmp", std::process::id()))
}

fn queue_lease(directory: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(directory.join("queue.lock"))?;
    // Serialize admission across plugin instances/processes as well as windows.
    file.lock()?;
    Ok(file)
}

fn persist(directory: &Path, report: &Value) -> io::Result<()> {
    let _lease = queue_lease(directory)?;
    let report_json = serde_json::to_string(report)?;
    if report_json.len() > REPORT_BYTES {
        return Err(io::Error::other(
            "report exceeds 64 KiB; export the local journal",
        ));
    }
    let report_id = digest(report_json.as_bytes());
    let path = directory.join(format!("{report_id}.json"));
    if path.exists() {
        return Ok(());
    }
    if pending_files(directory)?.len() >= PENDING {
        return Err(io::Error::other(
            "32 pending reports; export the local journal",
        ));
    }
    let body = serde_json::to_vec(
        &json!({ "schema": 1, "report_id": report_id, "report_json": report_json }),
    )?;
    let temporary = temporary_path(directory);
    let result = (|| {
        let mut file = super::private_file(&temporary)?;
        file.write_all(&body)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn acknowledged(reply: &Value, body: &Value) -> bool {
    let Some(url) = reply["issue_url"].as_str() else {
        return false;
    };
    let number = url
        .strip_prefix("https://github.com/Matari-Audio/MUI/issues/")
        .unwrap_or("");
    reply["ok"] == true
        && reply["report_id"] == body["report_id"]
        && !number.is_empty()
        && !number.starts_with('0')
        && number.bytes().all(|c| c.is_ascii_digit())
}

fn send(agent: &ureq::Agent, endpoint: &str, body: &[u8]) -> io::Result<Value> {
    if std::env::var_os("MUI_REPORTING_DISABLED").is_some_and(|value| value == "1") {
        return Err(io::Error::other("delivery disabled; local report retained"));
    }
    let mut response = agent
        .post(endpoint)
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|_| io::Error::other("support request failed; report retained"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(4096)
        .read_to_vec()
        .map_err(|_| io::Error::other("support response could not be read"))?;
    serde_json::from_slice(&bytes).map_err(io::Error::from)
}

fn deliver_pending(
    directory: &Path,
    stopped: &AtomicBool,
    mut send: impl FnMut(&[u8]) -> io::Result<Value>,
) -> bool {
    let Ok(files) = pending_files(directory) else {
        return false;
    };
    for path in files.into_iter().take(PENDING) {
        if stopped.load(Ordering::Acquire) {
            break;
        }
        let queued = (|| -> io::Result<(Vec<u8>, Value)> {
            let mut bytes = Vec::new();
            fs::File::open(&path)?
                .take((REPORT_BYTES * 2 + 4096) as u64)
                .read_to_end(&mut bytes)?;
            let body: Value = serde_json::from_slice(&bytes)?;
            let report_json = body["report_json"]
                .as_str()
                .ok_or_else(|| io::Error::other("invalid queued report"))?;
            if report_json.len() > REPORT_BYTES
                || body["schema"] != 1
                || body["report_id"] != digest(report_json.as_bytes())
            {
                return Err(io::Error::other("queued report failed integrity check"));
            }
            Ok((bytes, body))
        })();
        let (bytes, body) = match queued {
            Ok(queued) => queued,
            Err(error) => {
                // Preserve damaged evidence without blocking the other reports.
                eprintln!("MUI damaged queued report retained for export: {error}");
                continue;
            }
        };
        let result = (|| -> io::Result<()> {
            let reply = send(&bytes)?;
            if !acknowledged(&reply, &body) {
                return Err(io::Error::other(
                    "support did not acknowledge this exact report",
                ));
            }
            fs::remove_file(&path)?;
            super::breadcrumb(
                "mui",
                "report_delivered",
                reply["issue_url"].as_str().unwrap_or_default(),
            );
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("MUI automatic report: {error}");
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unanswered_dns_cancels_before_the_reporting_thread_can_exit() {
        use hickory_resolver::config::{NameServerConfig, ResolverConfig, ResolverOpts};
        let blackhole = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = blackhole.local_addr().unwrap();
        let mut nameserver = NameServerConfig::udp(address.ip());
        nameserver.connections[0].port = address.port();
        let mut dns = ResolverConfig::default();
        dns.add_name_server(nameserver);
        let resolver = JoinedResolver(dns, ResolverOpts::default());
        let config = ureq::Agent::config_builder().build();
        let start = Instant::now();
        let error = resolver
            .resolve(
                // .invalid is answered locally by RFC 6761; this name reaches the UDP fixture.
                &"https://unanswered.example.com/".parse().unwrap(),
                &config,
                NextTimeout {
                    after: ureq::unversioned::transport::time::Duration::from_millis(100),
                    reason: ureq::Timeout::Global,
                },
            )
            .unwrap_err();
        assert!(
            matches!(error, ureq::Error::Timeout(ureq::Timeout::Global)),
            "{error:?}"
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "DNS runtime must cancel and join"
        );
        blackhole
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut packet = [0; 512];
        assert!(
            blackhole.recv_from(&mut packet).unwrap().0 > 12,
            "a real DNS query reached the unanswered server"
        );
    }

    #[test]
    fn numeric_addresses_obey_port_and_ip_family_without_dns() {
        use hickory_resolver::config::{ResolverConfig, ResolverOpts};
        let resolver = JoinedResolver(ResolverConfig::default(), ResolverOpts::default());
        let config = ureq::Agent::config_builder()
            .ip_family(ureq::config::IpFamily::Ipv4Only)
            .build();
        let timeout = NextTimeout {
            after: ureq::unversioned::transport::time::Duration::from_millis(100),
            reason: ureq::Timeout::Global,
        };
        let addresses = resolver
            .resolve(&"http://127.0.0.1:8123/".parse().unwrap(), &config, timeout)
            .unwrap();
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0], "127.0.0.1:8123".parse().unwrap());
        let error = resolver
            .resolve(&"https://[::1]/".parse().unwrap(), &config, timeout)
            .unwrap_err();
        assert!(matches!(error, ureq::Error::HostNotFound));
    }

    #[test]
    fn native_window_guard_retains_worker_until_resource_teardown() {
        let directory = std::env::temp_dir().join(format!(
            "mui-reporting-lifetime-test-{}",
            std::process::id()
        ));
        let mut config = Config::new("lifetime-test", "1");
        config.directory = directory.clone();
        let reporter = Reporter::start(config).unwrap();
        let worker = Arc::downgrade(&reporter.0);
        let guard = super::super::retain_reporter();
        drop(reporter);
        assert!(
            worker.upgrade().is_some(),
            "native resources retain reporting"
        );
        drop(guard);
        assert!(
            worker.upgrade().is_none(),
            "final guard joins reporting worker"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn native_operation_recovery_ignores_live_and_completed_owners() {
        const CHILD_DIRECTORY: &str = "MUI_TEST_OPERATION_DIRECTORY";
        let report = json!({"component":"mui-vello","stage":"interrupted_request_device","message":"cause unconfirmed"});
        if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
            let _operation = Operation::create(Path::new(&directory), &report).unwrap();
            // Exit without Rust destructors, just as an abrupt native termination
            // releases the OS file lock without clearing the operation marker.
            std::process::exit(19);
        }
        let directory =
            std::env::temp_dir().join(format!("mui-native-operation-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let operation = Operation::create(&directory, &report).unwrap();
        recover_operations(&directory);
        assert!(
            pending_files(&directory).unwrap().is_empty(),
            "live owner must not be reported"
        );
        let path = operation.path.clone();
        drop(operation);
        assert!(!path.exists(), "normal completion removes its marker");
        let completed = directory.join("completed.active");
        {
            let mut file = super::super::private_file(&completed).unwrap();
            file.write_all(&[1]).unwrap();
            file.write_all(&serde_json::to_vec(&report).unwrap())
                .unwrap();
        }
        recover_operations(&directory);
        assert!(!completed.exists());
        assert!(
            pending_files(&directory).unwrap().is_empty(),
            "completed marker is never a crash report"
        );
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "diagnostics::reporting::tests::native_operation_recovery_ignores_live_and_completed_owners", "--nocapture"])
            .env(CHILD_DIRECTORY, &directory).status().unwrap();
        assert_eq!(child.code(), Some(19));
        recover_operations(&directory);
        let files = pending_files(&directory).unwrap();
        assert_eq!(files.len(), 1);
        let envelope: Value = serde_json::from_slice(&fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(envelope["report_json"].as_str().unwrap()).unwrap(),
            report
        );
        recover_operations(&directory);
        assert_eq!(
            pending_files(&directory).unwrap().len(),
            1,
            "recovery is idempotent"
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_and_wrong_acknowledgements_retain_the_exact_report_until_confirmed() {
        let directory =
            std::env::temp_dir().join(format!("mui-report-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let report = json!({"message": redact("gpu failed at C:\\Users\\private\\file with token=secret"), "app":"fixture"});
        persist(&directory, &report).unwrap();
        persist(&directory, &report).unwrap();
        let files = pending_files(&directory).unwrap();
        assert_eq!(files.len(), 1);
        let original = fs::read(&files[0]).unwrap();
        assert!(!String::from_utf8_lossy(&original).contains("secret"));
        let stopped = AtomicBool::new(false);
        assert!(!deliver_pending(&directory, &stopped, |_| Err(
            io::Error::other("offline")
        )));
        deliver_pending(&directory, &stopped, |_| {
            Ok(
                json!({"ok":true,"report_id":"wrong","issue_url":"https://github.com/Matari-Audio/MUI/issues/1"}),
            )
        });
        assert_eq!(fs::read(&files[0]).unwrap(), original);
        let corrupt = directory.join("000-corrupt.json");
        fs::write(&corrupt, b"damaged queued file").unwrap();
        deliver_pending(&directory, &stopped, |bytes| {
            let body: Value = serde_json::from_slice(bytes).unwrap();
            Ok(
                json!({"ok":true,"report_id":body["report_id"],"issue_url":"https://github.com/attacker/MUI/issues/1"}),
            )
        });
        assert_eq!(fs::read(&files[0]).unwrap(), original);
        deliver_pending(&directory, &stopped, |bytes| {
            assert_eq!(bytes, original);
            let body: Value = serde_json::from_slice(bytes).unwrap();
            Ok(
                json!({"ok":true,"report_id":body["report_id"],"issue_url":"https://github.com/Matari-Audio/MUI/issues/1"}),
            )
        });
        assert!(!files[0].exists());
        assert_eq!(fs::read(&corrupt).unwrap(), b"damaged queued file");
        fs::remove_dir_all(directory).unwrap();
    }
}
