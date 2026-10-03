// Adapted from BUFFR fd2fdba92f3f71cee24c0c72a39aa190fc9ee414; ISC, see LICENSE-BUFFR.
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(any(target_os = "macos", target_os = "windows", test))]
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct PlatformSnapshot {
    pub(super) os_name: String,
    pub(super) os_version: String,
    pub(super) kernel: String,
    pub(super) process_architecture: String,
    pub(super) native_architecture: String,
    pub(super) cpu: String,
    pub(super) logical_cores: usize,
    pub(super) memory_mib: u64,
    pub(super) graphics: String,
    pub(super) session: String,
    pub(super) runtime: String,
}

impl PlatformSnapshot {
    pub(super) fn render(&self) -> String {
        let mut output = String::new();
        push_field(&mut output, "Operating system", &self.os_name);
        push_field(&mut output, "OS version", &self.os_version);
        push_field(&mut output, "Kernel", &self.kernel);
        push_field(
            &mut output,
            "Process architecture",
            &self.process_architecture,
        );
        push_field(
            &mut output,
            "Native architecture",
            &self.native_architecture,
        );
        push_field(&mut output, "CPU", &self.cpu);
        if self.logical_cores > 0 {
            let _ = writeln!(output, "Logical processors: {}", self.logical_cores);
        }
        if self.memory_mib > 0 {
            let _ = writeln!(output, "Physical memory: {} MiB", self.memory_mib);
        }
        push_field(&mut output, "Graphics", &self.graphics);
        push_field(&mut output, "Desktop session", &self.session);
        push_field(&mut output, "Runtime", &self.runtime);
        output.trim_end().to_owned()
    }
}

fn push_field(output: &mut String, label: &str, value: &str) {
    if !value.is_empty() {
        let _ = writeln!(output, "{label}: {value}");
    }
}

static SNAPSHOT: OnceLock<PlatformSnapshot> = OnceLock::new();

pub(super) fn snapshot(stopping: &AtomicBool) -> &'static PlatformSnapshot {
    SNAPSHOT.get_or_init(|| capture_snapshot(stopping))
}

fn capture_snapshot(stopping: &AtomicBool) -> PlatformSnapshot {
    let mut snapshot = PlatformSnapshot {
        process_architecture: std::env::consts::ARCH.to_owned(),
        native_architecture: std::env::consts::ARCH.to_owned(),
        logical_cores: std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get),
        ..PlatformSnapshot::default()
    };

    if stopping.load(Ordering::Acquire) {
        return snapshot;
    }
    #[cfg(target_os = "linux")]
    capture_linux_snapshot(&mut snapshot);
    #[cfg(target_os = "macos")]
    capture_macos_snapshot(&mut snapshot, stopping);
    #[cfg(target_os = "windows")]
    capture_windows_snapshot(&mut snapshot);
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        snapshot.os_name = std::env::consts::OS.to_owned();
    }

    snapshot
}

#[cfg(target_os = "linux")]
fn capture_linux_snapshot(snapshot: &mut PlatformSnapshot) {
    let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    snapshot.os_name = os_release_value(&release, "PRETTY_NAME")
        .or_else(|| os_release_value(&release, "NAME"))
        .unwrap_or_else(|| "Linux".to_owned());
    snapshot.os_version = os_release_value(&release, "VERSION_ID").unwrap_or_default();
    snapshot.kernel = read_trimmed("/proc/sys/kernel/osrelease");
    snapshot.cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| {
            ["model name", "hardware", "processor"]
                .into_iter()
                .find_map(|key| colon_value(&text, key))
        })
        .unwrap_or_default();
    snapshot.memory_mib = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| colon_value(&text, "MemTotal"))
        .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
        .map_or(0, |kib| kib / 1024);
    snapshot.graphics = linux_graphics();
    snapshot.session = ["XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP"]
        .into_iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| format!("{name}={value}"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    snapshot.runtime = if std::env::var_os("WINELOADERNOEXEC").is_some()
        || std::env::var_os("WINEPREFIX").is_some()
    {
        "Wine".to_owned()
    } else {
        "native".to_owned()
    };
}

#[cfg(target_os = "linux")]
fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        (candidate == key).then(|| value.trim_matches(['\'', '"']).to_owned())
    })
}

#[cfg(target_os = "linux")]
fn colon_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        candidate
            .trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().to_owned())
    })
}

#[cfg(target_os = "linux")]
fn linux_graphics() -> String {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return String::new();
    };
    let mut adapters = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name
            .strip_prefix("card")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let uevent =
            std::fs::read_to_string(entry.path().join("device/uevent")).unwrap_or_default();
        let driver = equals_value(&uevent, "DRIVER").unwrap_or_default();
        let device = equals_value(&uevent, "PCI_ID").unwrap_or_default();
        let value = match (driver.is_empty(), device.is_empty()) {
            (false, false) => format!("{driver} ({device})"),
            (false, true) => driver,
            (true, false) => device,
            (true, true) => continue,
        };
        if !adapters.contains(&value) {
            adapters.push(value);
        }
        if adapters.len() == 3 {
            break;
        }
    }
    adapters.join(", ")
}

#[cfg(target_os = "linux")]
fn equals_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        (candidate == key).then(|| value.trim().to_owned())
    })
}

#[cfg(target_os = "macos")]
fn capture_macos_snapshot(snapshot: &mut PlatformSnapshot, stopping: &AtomicBool) {
    snapshot.os_name = "macOS".to_owned();
    snapshot.os_version = command_output("sw_vers", &["-productVersion"], stopping);
    snapshot.kernel = command_output("uname", &["-sr"], stopping);
    snapshot.cpu = command_output("sysctl", &["-n", "machdep.cpu.brand_string"], stopping);
    if snapshot.cpu.is_empty() {
        snapshot.cpu = command_output("sysctl", &["-n", "hw.model"], stopping);
    }
    snapshot.memory_mib = command_output("sysctl", &["-n", "hw.memsize"], stopping)
        .parse::<u64>()
        .map(|bytes| bytes / (1024 * 1024))
        .unwrap_or(0);
    let translated = command_output("sysctl", &["-n", "sysctl.proc_translated"], stopping) == "1";
    snapshot.runtime = if translated {
        "Rosetta 2".to_owned()
    } else {
        "native".to_owned()
    };
    if translated {
        snapshot.native_architecture = "aarch64".to_owned();
    }
}

#[cfg(target_os = "macos")]
fn command_output(program: &str, args: &[&str], stopping: &AtomicBool) -> String {
    command_output_with_deadline(
        std::process::Command::new(program).args(args),
        std::time::Duration::from_secs(2),
        stopping,
    )
    .ok()
    .flatten()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    .unwrap_or_default()
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn command_output_with_deadline(
    command: &mut std::process::Command,
    limit: std::time::Duration,
    stopping: &AtomicBool,
) -> std::io::Result<Option<std::process::Output>> {
    use std::io::{Read, Seek, SeekFrom};
    use std::process::Stdio;
    // Anonymous files let the child finish even when its output exceeds a pipe buffer.
    if stopping.load(Ordering::Acquire) {
        return Ok(None);
    }
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .spawn()?;
    let started = std::time::Instant::now();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        match status {
            Some(status) => {
                if stopping.load(Ordering::Acquire) {
                    return Ok(None);
                }
                stdout.seek(SeekFrom::Start(0))?;
                stderr.seek(SeekFrom::Start(0))?;
                let mut stdout_bytes = Vec::new();
                let mut stderr_bytes = Vec::new();
                stdout.take(1_000_000).read_to_end(&mut stdout_bytes)?;
                stderr.take(1_000_000).read_to_end(&mut stderr_bytes)?;
                return Ok(Some(std::process::Output {
                    status,
                    stdout: stdout_bytes,
                    stderr: stderr_bytes,
                }));
            }
            None if !stopping.load(Ordering::Acquire) && started.elapsed() < limit => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            None => {
                let _ = child.kill();
                // Reap before the host may unload the module. Kernel-stuck children can delay it.
                let _ = child.wait();
                return Ok(None);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn capture_windows_snapshot(snapshot: &mut PlatformSnapshot) {
    use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
    use windows_sys::Win32::System::SystemInformation::{
        GetNativeSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, OSVERSIONINFOW,
        PROCESSOR_ARCHITECTURE_AMD64, PROCESSOR_ARCHITECTURE_ARM64, PROCESSOR_ARCHITECTURE_INTEL,
        SYSTEM_INFO,
    };

    snapshot.os_name = "Windows".to_owned();
    let mut version = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..OSVERSIONINFOW::default()
    };
    // SAFETY: `version` is initialized with the required size and points to writable memory.
    if unsafe { RtlGetVersion(&mut version) } >= 0 {
        snapshot.os_version = format!(
            "{}.{}.{}",
            version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber
        );
        snapshot.kernel = format!("NT {}", snapshot.os_version);
    }

    let mut info = SYSTEM_INFO::default();
    // SAFETY: `info` points to valid writable memory for the duration of the call.
    unsafe { GetNativeSystemInfo(&mut info) };
    // SAFETY: `GetNativeSystemInfo` initialized the active union member documented for SYSTEM_INFO.
    let architecture = unsafe { info.Anonymous.Anonymous.wProcessorArchitecture };
    snapshot.native_architecture = match architecture {
        PROCESSOR_ARCHITECTURE_AMD64 => "x86_64",
        PROCESSOR_ARCHITECTURE_ARM64 => "aarch64",
        PROCESSOR_ARCHITECTURE_INTEL => "x86",
        _ => "unknown",
    }
    .to_owned();
    snapshot.logical_cores = info.dwNumberOfProcessors as usize;
    snapshot.cpu = windows_cpu_name();
    snapshot.session = std::env::var("SESSIONNAME").unwrap_or_default();

    let mut memory = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..MEMORYSTATUSEX::default()
    };
    // SAFETY: `memory` is initialized with the required size and points to writable memory.
    if unsafe { GlobalMemoryStatusEx(&mut memory) } != 0 {
        snapshot.memory_mib = memory.ullTotalPhys / (1024 * 1024);
    }
    snapshot.runtime = if std::env::var_os("WINELOADERNOEXEC").is_some()
        || std::env::var_os("WINEPREFIX").is_some()
    {
        "Wine".to_owned()
    } else {
        "native".to_owned()
    };
}

#[cfg(target_os = "windows")]
fn windows_cpu_name() -> String {
    #[cfg(target_arch = "x86")]
    use std::arch::x86::__cpuid;
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::__cpuid;

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        let maximum = __cpuid(0x8000_0000).eax;
        if maximum >= 0x8000_0004 {
            let mut bytes = Vec::with_capacity(48);
            for leaf in 0x8000_0002..=0x8000_0004 {
                let result = __cpuid(leaf);
                for word in [result.eax, result.ebx, result.ecx, result.edx] {
                    bytes.extend_from_slice(&word.to_le_bytes());
                }
            }
            let brand = String::from_utf8_lossy(&bytes)
                .trim_matches(['\0', ' '])
                .to_owned();
            if !brand.is_empty() {
                return brand;
            }
        }
    }
    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum EvidenceDisposition {
    #[default]
    None,
    Crash,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CrashEvidence {
    pub(super) disposition: EvidenceDisposition,
    pub(super) signature: String,
    pub(super) text: String,
}

pub(super) fn collect_crash_evidence(
    host_process: &str,
    pid: u32,
    started_at: u64,
    ended_at: u64,
    stopping: &AtomicBool,
) -> CrashEvidence {
    if stopping.load(Ordering::Acquire) {
        return CrashEvidence::default();
    }
    #[cfg(target_os = "linux")]
    return collect_linux_crash_evidence(host_process, pid, started_at, ended_at, stopping);
    #[cfg(target_os = "macos")]
    return collect_macos_crash_evidence(host_process, pid, started_at, ended_at, stopping);
    #[cfg(target_os = "windows")]
    return collect_windows_crash_evidence(host_process, pid, started_at, ended_at, stopping);
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (host_process, pid, started_at, ended_at, stopping);
        CrashEvidence::default()
    }
}

#[cfg(target_os = "linux")]
fn collect_linux_crash_evidence(
    host_process: &str,
    pid: u32,
    started_at: u64,
    ended_at: u64,
    stopping: &AtomicBool,
) -> CrashEvidence {
    let mut evidence = CrashEvidence::default();

    match command_output_with_deadline(
        std::process::Command::new("coredumpctl")
            .env("LC_ALL", "C")
            .args(linux_coredumpctl_args(pid, started_at, ended_at)),
        std::time::Duration::from_secs(2),
        stopping,
    ) {
        Ok(Some(output)) => {
            let text = String::from_utf8_lossy(&output.stdout);
            if output.status.success() && linux_coredump_matches_host(&text, host_process) {
                evidence.disposition = EvidenceDisposition::Crash;
                evidence.signature = selected_signature_lines(
                    &text,
                    &["signal:", "command line:", "executable:", "storage:"],
                );
                append_evidence(&mut evidence.text, "Correlated systemd-coredump", &text);
            } else {
                evidence.text = format!(
                    "Native crash-report collection was incomplete ({}):\n{}\n{}",
                    output.status,
                    text,
                    String::from_utf8_lossy(&output.stderr),
                );
            }
        }
        Ok(None) => {
            evidence.text = "Native crash-report collection stopped or timed out.".to_owned()
        }
        Err(error) => evidence.text = format!("Native crash-report collection failed: {error}"),
    }
    evidence
}

#[cfg(any(target_os = "linux", test))]
fn linux_coredump_matches_host(text: &str, host_process: &str) -> bool {
    let expected = comparable_process_name(host_process);
    let mut executables = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Executable:"))
        .map(|path| {
            comparable_process_name(
                path.trim()
                    .strip_suffix(" (deleted)")
                    .unwrap_or(path.trim()),
            )
        });
    !expected.is_empty()
        && executables.next().is_some_and(|actual| actual == expected)
        && executables.all(|actual| actual == expected)
}

#[cfg(any(target_os = "linux", test))]
fn linux_coredumpctl_args(pid: u32, started_at: u64, ended_at: u64) -> [String; 5] {
    [
        "--no-pager".to_string(),
        format!("--since=@{}", started_at.saturating_sub(60)),
        format!("--until=@{}", ended_at.max(started_at).saturating_add(120)),
        "info".to_string(),
        pid.to_string(),
    ]
}

#[cfg(target_os = "macos")]
fn collect_macos_crash_evidence(
    host_process: &str,
    pid: u32,
    started_at: u64,
    ended_at: u64,
    stopping: &AtomicBool,
) -> CrashEvidence {
    let Some(home) = std::env::var_os("HOME") else {
        return CrashEvidence::default();
    };
    let directory = PathBuf::from(home)
        .join("Library")
        .join("Logs")
        .join("DiagnosticReports");
    let mut reports = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .take_while(|_| !stopping.load(Ordering::Acquire))
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
            let modified = modified_unix(&path)?;
            ((name.ends_with(".ips") || name.ends_with(".crash"))
                && artifact_matches_host(&path, host_process)
                && within_incident_window(modified, started_at, ended_at))
            .then_some((modified, path))
        })
        .collect::<Vec<_>>();
    reports.sort_by_key(|(modified, _)| *modified);
    macos_evidence_from_paths(
        pid,
        host_process,
        reports.into_iter().rev().map(|(_, path)| path),
        stopping,
    )
}

#[cfg(any(target_os = "macos", test))]
fn macos_evidence_from_paths(
    pid: u32,
    host_process: &str,
    reports: impl IntoIterator<Item = PathBuf>,
    stopping: &AtomicBool,
) -> CrashEvidence {
    let mut omitted = false;
    for path in reports {
        if stopping.load(Ordering::Acquire) {
            return CrashEvidence::default();
        }
        let (report, original_bytes, original_hash) = match read_report_file(&path, stopping) {
            Ok(report) => report,
            Err(_) => {
                omitted = true;
                continue;
            }
        };
        if !macos_report_matches_host(&report, host_process) {
            continue;
        }
        let Some(mut summary) = macos_summary_for_pid(pid, [report]) else {
            continue;
        };
        match super::crash::preserve_original_file(
            &path,
            "macos-native-report-exact-pid",
            stopping,
            Some((original_bytes, original_hash.as_str())),
        ) {
            Ok(status) => {
                let _ = write!(summary, "\n{status}");
            }
            Err(error) => {
                let _ = write!(
                    summary,
                    "\nComplete native original could not be privately archived ({error}); the OS source remains in place."
                );
            }
        }
        if omitted {
            summary.push_str("\nAdditional unreadable/oversized matching-name candidates were omitted without PID validation; no content or stack from those candidates is included.");
        }
        return CrashEvidence {
            disposition: EvidenceDisposition::Crash,
            signature: selected_signature_lines(
                &summary,
                &["exception", "termination", "faultingthread", "signal"],
            ),
            text: format!("Correlated macOS diagnostic report:\n{summary}"),
        };
    }
    CrashEvidence {
        text: if omitted {
            "Native candidate evidence was unreadable or exceeded the 8 MiB buffered-read limit. No complete report could be parsed and PID-confirmed; no stack was fabricated or candidate contents uploaded. Complete oversized candidates remain in private local archival storage when archival succeeds.".into()
        } else {
            String::new()
        },
        ..Default::default()
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_report_matches_host(text: &str, host: &str) -> bool {
    let actual = if text.trim_start().starts_with('{') {
        let Some((_, body)) = text.split_once('\n') else {
            return false;
        };
        let Ok(body) = serde_json::from_str::<serde_json::Value>(body) else {
            return false;
        };
        body.get("procName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    } else {
        text.lines()
            .find_map(|line| line.strip_prefix("Process:"))
            .and_then(|value| value.rsplit_once('['))
            .map(|(name, _)| name.trim().to_owned())
            .unwrap_or_default()
    };
    let expected = comparable_process_name(host);
    !expected.is_empty() && comparable_process_name(&actual) == expected
}

#[cfg(any(target_os = "macos", test))]
fn macos_summary_for_pid(pid: u32, reports: impl IntoIterator<Item = String>) -> Option<String> {
    reports.into_iter().find_map(|report| {
        let (report_pid, summary) = macos_crash_summary(&report)?;
        if report_pid != u64::from(pid) { return None; }
        if !report.trim_start().starts_with('{') { return Some(bounded_native_text(&summary)); }
        // Sanitize the parsed .ips before bounded pretty rendering so a personal
        // path cannot cause token redaction to hide the actual stack frames.
        let full = report.split_once('\n').and_then(|(header, body)| {
            let mut header:serde_json::Value = serde_json::from_str(header).ok()?;
            let mut body:serde_json::Value = serde_json::from_str(body).ok()?;
            super::sanitize_automatic(&mut header);
            super::sanitize_automatic(&mut body);
            Some(format!("{}\n{}",super::report::bounded_pretty_json(&header,1024*1024)?,super::report::bounded_pretty_json(&body,1024*1024)?))
        }).unwrap_or_else(|| "PARTIAL NATIVE REPORT: complete JSON could not be rendered within the bounded display budget. Validated summary is included; complete raw original remains local.".into());
        Some(format!("{summary}\n\nCaptured macOS report:\n{full}"))
    })
}

#[cfg(any(target_os = "macos", test))]
fn macos_crash_summary(text: &str) -> Option<(u64, String)> {
    if !text.trim_start().starts_with('{') {
        // Older macOS releases write a text .crash report. Match the exact PID,
        // retain the exception and all frames; a host name alone is insufficient.
        let process = text.lines().find(|line| line.starts_with("Process:"))?;
        let pid = process
            .rsplit_once('[')?
            .1
            .trim_end_matches(']')
            .trim()
            .parse::<u64>()
            .ok()?;
        if text.lines().any(|line| {
            line.starts_with("Exception Note:")
                && (line.contains("NON-FATAL") || line.contains("SIMULATED"))
        }) {
            return None;
        }
        if !text.lines().any(|line| {
            line.strip_prefix("Exception Type:")
                .is_some_and(|value| !value.trim().is_empty())
        }) {
            return None;
        }
        return Some((pid, text.to_owned()));
    }
    let (header, report) = text.split_once('\n')?;
    let header = serde_json::from_str::<serde_json::Value>(header).ok()?;
    let report = serde_json::from_str::<serde_json::Value>(report).ok()?;
    let pid = report.get("pid")?.as_u64()?;
    // Apple documents type309 as this crash schema and explicitly marks
    // nonfatal/simulated conditions. PID or termination namespace alone cannot
    // establish a fatal crash. See the primary reference in README.
    if header.get("bug_type").and_then(serde_json::Value::as_str) != Some("309")
        && header.get("bug_type").and_then(serde_json::Value::as_u64) != Some(309)
    {
        return None;
    }
    for value in [&header, &report] {
        for key in ["isNonFatal", "isSimulated", "is_non_fatal", "is_simulated"] {
            if let Some(flag) = value.get(key) {
                let explicitly_false = flag.as_bool() == Some(false)
                    || flag.as_i64() == Some(0)
                    || flag
                        .as_str()
                        .is_some_and(|v| v.eq_ignore_ascii_case("false") || v == "0");
                if !explicitly_false {
                    return None;
                }
            }
        }
    }
    if !report.get("exception").is_some_and(|exception| {
        ["type", "signal"].into_iter().any(|key| {
            exception
                .get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|v| !v.trim().is_empty())
        })
    }) {
        return None;
    }
    let mut summary = serde_json::Map::new();
    for key in [
        "app_name",
        "app_version",
        "build_version",
        "os_version",
        "timestamp",
        "bug_type",
    ] {
        if let Some(value) = header.get(key) {
            summary.insert(key.to_owned(), value.clone());
        }
    }
    for key in [
        "procName",
        "pid",
        "captureTime",
        "procLaunch",
        "cpuType",
        "translated",
        "uptime",
        "exception",
        "termination",
        "asi",
        "lastExceptionBacktrace",
        "ktriageinfo",
        "faultingThread",
        "vmSummary",
    ] {
        if let Some(value) = report.get(key) {
            summary.insert(key.to_owned(), value.clone());
        }
    }

    if let Some(thread_index) = report
        .get("faultingThread")
        .and_then(serde_json::Value::as_u64)
        && let Some(thread) = report
            .get("threads")
            .and_then(serde_json::Value::as_array)
            .and_then(|threads| threads.get(thread_index as usize))
    {
        if let Some(queue) = thread.get("queue") {
            summary.insert("faultingQueue".to_owned(), queue.clone());
        }
        let images = report
            .get("usedImages")
            .and_then(serde_json::Value::as_array);
        let frames = thread
            .get("frames")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .take(24)
            .map(|frame| {
                let mut selected = serde_json::Map::new();
                for key in ["symbol", "symbolLocation", "imageOffset"] {
                    if let Some(value) = frame.get(key) {
                        selected.insert(key.to_owned(), value.clone());
                    }
                }
                if let Some(image_index) =
                    frame.get("imageIndex").and_then(serde_json::Value::as_u64)
                    && let Some(image) = images.and_then(|images| images.get(image_index as usize))
                    && let Some(name) = image.get("name")
                {
                    selected.insert("image".to_owned(), name.clone());
                }
                serde_json::Value::Object(selected)
            })
            .collect();
        summary.insert(
            "faultingFrames".to_owned(),
            serde_json::Value::Array(frames),
        );
    }
    let mut summary = serde_json::Value::Object(summary);
    super::sanitize_automatic(&mut summary);
    if let Some(rendered) = super::report::bounded_pretty_json(&summary, 1024 * 1024) {
        return Some((pid, rendered));
    }
    // Keep the actual exception and faulting-frame context when large optional
    // diagnostic sections exceed the display budget. Never reconstruct frames.
    for key in ["asi", "vmSummary", "ktriageinfo", "lastExceptionBacktrace"] {
        summary.as_object_mut().unwrap().remove(key);
    }
    let rendered = super::report::bounded_pretty_json(&summary,1024*1024)
        .unwrap_or_else(|| format!("Validated native crash PID: {pid}. Exception/frame display also exceeded its budget; no reconstructed stack is included."));
    Some((
        pid,
        format!(
            "PARTIAL NATIVE SUMMARY: oversized optional diagnostic sections were omitted; complete raw original remains local.\n{rendered}"
        ),
    ))
}

#[cfg(target_os = "windows")]
fn collect_windows_crash_evidence(
    host_process: &str,
    pid: u32,
    started_at: u64,
    ended_at: u64,
    stopping: &AtomicBool,
) -> CrashEvidence {
    let mut evidence = CrashEvidence::default();
    let mut dump_at = None;
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let directory = PathBuf::from(local_app_data).join("CrashDumps");
        let mut dumps = std::fs::read_dir(directory)
            .into_iter()
            .flatten()
            .flatten()
            .take_while(|_| !stopping.load(Ordering::Acquire))
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_string_lossy().into_owned();
                let metadata = std::fs::metadata(&path).ok()?;
                let modified = modified_unix(&path)?;
                (name.to_ascii_lowercase().ends_with(".dmp")
                    && artifact_matches_host(&path, host_process)
                    && dump_matches_pid(&name, pid)
                    && within_incident_window(modified, started_at, ended_at))
                .then_some((modified, name, metadata.len()))
            })
            .collect::<Vec<_>>();
        dumps.sort_by_key(|(modified, _, _)| *modified);
        if let Some((modified, name, bytes)) = dumps.pop() {
            dump_at = Some(modified);
            evidence.disposition = EvidenceDisposition::Crash;
            evidence.signature = "windows-local-dump".to_owned();
            evidence.text = format!(
                "Correlated Windows crash dump available locally: {name} ({bytes} bytes). Binary dumps are not uploaded automatically."
            );
        }
    }

    let Some(dump_at) = dump_at else {
        return evidence;
    };
    let Some(program_data) = std::env::var_os("PROGRAMDATA") else {
        return evidence;
    };
    let root = PathBuf::from(program_data)
        .join("Microsoft")
        .join("Windows")
        .join("WER");
    let mut reports = ["ReportArchive", "ReportQueue"]
        .into_iter()
        .flat_map(|name| std::fs::read_dir(root.join(name)).into_iter().flatten())
        .flatten()
        .take_while(|_| !stopping.load(Ordering::Acquire))
        .filter_map(|entry| {
            let directory = entry.path();
            if !artifact_matches_host(&directory, host_process) {
                return None;
            }
            let report = directory.join("Report.wer");
            let modified = modified_unix(&report)?;
            (modified.abs_diff(dump_at) <= 10).then_some((modified, report))
        })
        .collect::<Vec<_>>();
    reports.sort_by_key(|(modified, _)| *modified);
    if let Some((_, path)) = reports.pop() {
        match read_report_file(&path, stopping) {
            Ok((text, original_bytes, original_hash))
                if windows_wer_matches_host(&text, host_process) =>
            {
                append_evidence(
                    &mut evidence.text,
                    "Windows Error Reporting record matched to the PID-specific dump time and application",
                    &bounded_native_text(&text),
                );
                if let Ok(status) = super::crash::preserve_original_file(
                    &path,
                    "windows-wer-matched-application-and-dump",
                    stopping,
                    Some((original_bytes, original_hash.as_str())),
                ) {
                    append_evidence(&mut evidence.text, "Private original", &status);
                }
            }
            Ok(_) => append_evidence(
                &mut evidence.text,
                "WER omitted",
                "The candidate did not identify the recorded application; its contents were not included.",
            ),
            Err(_) => append_evidence(
                &mut evidence.text,
                "PARTIAL NATIVE REPORT",
                "WER text could not be completely read within the 8 MiB limit. No WER exception or stack was inferred; the PID-specific binary dump still confirms the crash. Oversized WER candidates remain private when archival succeeds.",
            ),
        }
    }
    evidence
}

#[cfg(target_os = "windows")]
fn dump_matches_pid(name: &str, pid: u32) -> bool {
    let needle = format!(".{pid}.");
    name.contains(&needle) || name.ends_with(&format!(".{pid}.dmp"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn artifact_matches_host(path: &Path, host_process: &str) -> bool {
    let host = comparable_process_name(host_process);
    let artifact = path
        .file_name()
        .map(|name| comparable_process_name(&name.to_string_lossy()))
        .unwrap_or_default();
    !host.is_empty() && artifact.contains(&host)
}

/// File stem, lowercased, punctuation removed, so `reaper.exe`, `REAPER` and
/// `reaper` compare equal while `FL64.exe` does not.
pub(super) fn comparable_process_name(value: &str) -> String {
    Path::new(value)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn modified_unix(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn within_incident_window(modified: u64, started_at: u64, ended_at: u64) -> bool {
    (started_at.saturating_sub(60)..=ended_at.max(started_at).saturating_add(120))
        .contains(&modified)
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn read_report_file(path: &Path, stopping: &AtomicBool) -> Result<(String, u64, String), String> {
    if stopping.load(Ordering::Acquire) {
        return Err("Native collection stopped".into());
    }
    match super::read_bounded_file(path, super::LOCAL_REPORT_BYTES) {
        Ok(Some(bytes)) => Ok((
            decode_report_text(&bytes),
            bytes.len() as u64,
            blake3::hash(&bytes).to_hex().to_string(),
        )),
        Ok(None) => {
            let status = super::crash::preserve_original_file(path, "oversized-native-candidate-unparsed-unverified", stopping, None)
                .unwrap_or_else(|error| format!("Oversized native candidate remains in OS storage; private archival failed: {error}"));
            crate::diagnostics::event(
                crate::diagnostics::LogLevel::Warning,
                "support",
                "oversized_native_candidate_unparsed",
                serde_json::json!({"reason":format!("{status} Candidate contents were not parsed or used to confirm a crash, and were not uploaded.")}),
            );
            Err(status)
        }
        Err(error) => Err(format!("Native candidate read failed: {error}")),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn bounded_native_text(text: &str) -> String {
    const LIMIT: usize = 1024 * 1024;
    if text.len() <= LIMIT {
        return text.into();
    }
    let mut head = LIMIT / 2;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - LIMIT / 2;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n[PARTIAL NATIVE REPORT: {} decoded UTF-8 bytes omitted from the middle; these are actual prefix/suffix bytes, not reconstructed frames. Complete original remains local.]\n{}",
        &text[..head],
        tail - head,
        &text[tail..]
    )
}

#[cfg(any(target_os = "windows", test))]
fn windows_wer_matches_host(text: &str, host: &str) -> bool {
    let expected = comparable_process_name(host);
    let mut found = false;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if matches!(key.trim(), "AppPath" | "AppName") {
                let basename = value
                    .trim()
                    .trim_matches('"')
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or_default();
                if expected.is_empty() || comparable_process_name(basename) != expected {
                    return false;
                }
                found = true;
            }
        }
    }
    found
}

/// Windows Error Reporting writes `Report.wer` as UTF-16LE with a byte-order
/// mark; treating that as UTF-8 yields NUL-riddled text that wastes the
/// evidence budget. Everything else is read as (lossy) UTF-8.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn decode_report_text(bytes: &[u8]) -> String {
    let utf16 = |rest: &[u8], from_bytes: fn([u8; 2]) -> u16| {
        char::decode_utf16(rest.as_chunks::<2>().0.iter().copied().map(from_bytes))
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect::<String>()
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xfe, 0xff, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn selected_signature_lines(text: &str, markers: &[&str]) -> String {
    let selected = text
        .lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            markers.iter().any(|marker| lower.contains(marker))
        })
        .take(24)
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("|");
    if selected.is_empty() {
        "platform-crash".to_owned()
    } else {
        normalize_signature(&selected)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn normalize_signature(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len().min(512));
    let mut number = false;
    for character in value.chars().take(1024) {
        if character.is_ascii_digit() {
            if !number {
                normalized.push('#');
                number = true;
            }
        } else {
            number = false;
            normalized.push(character.to_ascii_lowercase());
        }
        if normalized.len() >= 512 {
            break;
        }
    }
    normalized
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn append_evidence(output: &mut String, label: &str, text: &str) {
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    let _ = write!(output, "{label}:\n{text}");
}

#[cfg(target_os = "linux")]
fn read_trimmed(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path)
        .map(|value| value.trim().to_owned())
        .unwrap_or_default()
}

/// Whether `pid` is still the process that wrote a session marker for `host_process`.
///
/// A live pid alone is not enough: pids are reused, and a reused pid used to hide the crash that
/// freed it. A process whose image name cannot be read is assumed to still be the host.
pub(super) fn process_is_alive(pid: u32, host_process: &str) -> bool {
    let host = comparable_process_name(host_process);
    let is_host = |image: Option<String>| {
        image.is_none_or(|image| host.is_empty() || comparable_process_name(&image) == host)
    };
    #[cfg(target_os = "linux")]
    {
        let proc_dir = Path::new("/proc").join(pid.to_string());
        pid != 0
            && proc_dir.exists()
            && is_host(std::fs::read_link(proc_dir.join("exe")).ok().map(|path| {
                // A host updated while running links to `reaper (deleted)`.
                let path = path.to_string_lossy().into_owned();
                path.strip_suffix(" (deleted)").unwrap_or(&path).to_owned()
            }))
    }
    #[cfg(target_os = "macos")]
    {
        // A pid above `pid_t::MAX` would cast to a negative value, which `kill` reads as a
        // process group.
        let Some(pid) = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0) else {
            return false;
        };
        // SAFETY: signal 0 only checks process existence and permission.
        let result = unsafe { libc::kill(pid, 0) };
        if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            return false;
        }
        let mut path = [0_u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: `path` is writable for the length passed.
        let length =
            unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        is_host(
            usize::try_from(length)
                .ok()
                .filter(|length| *length > 0)
                .map(|length| String::from_utf8_lossy(&path[..length]).into_owned()),
        )
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        };

        // SAFETY: the returned handle is checked for null and closed exactly once below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return false;
        }
        let mut exit_code = 0;
        // SAFETY: `process` is a valid open handle and `exit_code` is writable.
        let success = unsafe { GetExitCodeProcess(process, &mut exit_code) } != 0;
        let mut path = [0_u16; 1024];
        let mut length = path.len() as u32;
        // SAFETY: `process` is a valid open handle; `path` is writable for `length` units.
        let named = unsafe {
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut length)
        } != 0;
        // SAFETY: `process` is a valid owned handle and is not used after this call.
        let _ = unsafe { CloseHandle(process) };
        success
            && exit_code == STILL_ACTIVE as u32
            && is_host(named.then(|| String::from_utf16_lossy(&path[..length as usize])))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (pid, is_host);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_capture_keeps_full_private_originals_without_confirming_oversized_or_wrong_pid_candidates()
     {
        const CHILD: &str = "KONTRA_NATIVE_CAPTURE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let directory = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","support::platform::tests::native_capture_keeps_full_private_originals_without_confirming_oversized_or_wrong_pid_candidates","--test-threads=1"])
                .env(CHILD,"1").env("KONTRA_REPORT_DIR",directory.path()).env("KONTRA_DISABLE_NETWORK","1").status().unwrap();
            assert!(status.success());
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let oversized = directory.path().join("Host-oversized.crash");
        let raw = format!(
            "Process: Host [42]\nException Type: EXC_BAD_ACCESS\n{}",
            "unverified-private-candidate ".repeat(super::super::LOCAL_REPORT_BYTES / 20)
        );
        assert!(raw.len() > super::super::LOCAL_REPORT_BYTES);
        std::fs::write(&oversized, &raw).unwrap();
        let wrong_pid = directory.path().join("Host-wrong.crash");
        std::fs::write(
            &wrong_pid,
            "Process: Host [43]\nException Type: EXC_BAD_ACCESS\nwrong-pid-private-frames",
        )
        .unwrap();
        let matching = directory.path().join("Host-matching.crash");
        let matching_raw = "Process: Host [42]\nException Type: EXC_BAD_ACCESS\n0 KONTRA authored_native_frame + 12";
        std::fs::write(&matching, matching_raw).unwrap();
        let stopping = AtomicBool::new(false);
        let unconfirmed = macos_evidence_from_paths(
            42,
            "Host",
            [oversized.clone(), wrong_pid.clone()],
            &stopping,
        );
        assert_eq!(unconfirmed.disposition, EvidenceDisposition::None);
        assert!(
            unconfirmed
                .text
                .contains("No complete report could be parsed and PID-confirmed")
        );
        assert!(
            !unconfirmed.text.contains("unverified-private-candidate")
                && !unconfirmed.text.contains("wrong-pid-private-frames")
        );
        let confirmed = macos_evidence_from_paths(
            42,
            "Host",
            [oversized.clone(), wrong_pid, matching.clone()],
            &stopping,
        );
        assert_eq!(confirmed.disposition, EvidenceDisposition::Crash);
        assert!(
            confirmed.text.contains("authored_native_frame + 12")
                && confirmed.text.contains("omitted without PID validation")
        );
        assert!(
            !confirmed.text.contains("unverified-private-candidate")
                && !confirmed.text.contains("wrong-pid-private-frames")
        );
        let originals = super::super::support_cache_path()
            .parent()
            .unwrap()
            .join("crash-reports/originals");
        for bytes in [raw.as_bytes(), matching_raw.as_bytes()] {
            let hash = blake3::hash(bytes).to_hex().to_string();
            let archive = originals.join(format!("{hash}.raw"));
            assert_eq!(std::fs::read(&archive).unwrap(), bytes);
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(archive.with_extension("json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["automatically_uploaded"], false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&archive).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        assert_eq!(std::fs::read(&oversized).unwrap(), raw.as_bytes());
        let (_, bytes, hash) = read_report_file(&matching, &stopping).unwrap();
        std::fs::write(&matching, b"different private source after parsing").unwrap();
        assert!(
            super::super::crash::preserve_original_file(
                &matching,
                "macos-native-report-exact-pid",
                &stopping,
                Some((bytes, &hash))
            )
            .is_err()
        );
        assert_eq!(
            macos_evidence_from_paths(42, "Host", [matching], &AtomicBool::new(true)).disposition,
            EvidenceDisposition::None
        );
    }

    #[test]
    fn bounded_native_rendering_is_explicit_and_windows_wer_requires_the_actual_application() {
        let text = format!(
            "Exception Type: EXC_BAD_ACCESS\n{}\nactual last frame",
            "😀".repeat(300_000)
        );
        let bounded = bounded_native_text(&text);
        assert!(
            bounded.contains("PARTIAL NATIVE REPORT")
                && bounded.contains("Exception Type: EXC_BAD_ACCESS")
                && bounded.contains("actual last frame")
        );
        assert!(bounded.len() < 1024 * 1024 + 512);
        assert!(
            super::super::report::bounded_pretty_json(&serde_json::json!({"payload":text}), 128)
                .is_none()
        );
        assert!(windows_wer_matches_host(
            "AppPath=C:\\Program Files\\REAPER.exe\nAppName=reaper.exe",
            "reaper"
        ));
        assert!(!windows_wer_matches_host(
            "AppPath=C:/Host/StudioOne.exe",
            "reaper"
        ));
        assert!(!windows_wer_matches_host(
            "AppName=reaper.exe\nAppPath=C:/Host/Other.exe",
            "reaper"
        ));
        assert!(!windows_wer_matches_host("EventType=APPCRASH", "reaper"));
        let header = serde_json::json!({"bug_type":"309"});
        let body = serde_json::json!({"pid":42,"procName":"Host","exception":{"type":"EXC_BAD_ACCESS","signal":"SIGSEGV"},"vmSummary":"x".repeat(1024*1024+1),"faultingThread":0,"threads":[{"frames":[{"symbol":"actual_authored_frame"}]}]});
        let (_, summary) = macos_crash_summary(&format!("{header}\n{body}")).unwrap();
        assert!(
            summary.contains("PARTIAL NATIVE SUMMARY")
                && summary.contains("actual_authored_frame")
                && summary.contains("EXC_BAD_ACCESS")
        );
    }

    #[test]
    fn apple_crash_schema_excludes_normal_stackshot_nonfatal_and_simulated_reports() {
        let header = serde_json::json!({"app_name":"REAPER","bug_type":"309"});
        let body = serde_json::json!({"pid":42,"procName":"REAPER","exception":{"type":"EXC_BAD_ACCESS","signal":"SIGSEGV"},"termination":{"namespace":"SIGNAL","code":11},"faultingThread":0,"threads":[{"frames":[{"symbol":"authored_native_crash"}]}]});
        let report =
            |header: &serde_json::Value, body: &serde_json::Value| format!("{header}\n{body}");
        assert!(
            macos_crash_summary(&report(&header, &body))
                .unwrap()
                .1
                .contains("authored_native_crash")
        );
        assert!(macos_report_matches_host(&report(&header, &body), "reaper"));
        assert!(!macos_report_matches_host(
            &report(&header, &body),
            "StudioOne"
        ));
        for flag in ["isNonFatal", "isSimulated"] {
            for value in [
                serde_json::json!(true),
                serde_json::json!("true"),
                serde_json::json!(1),
            ] {
                let mut diagnostic = body.clone();
                diagnostic[flag] = value;
                assert!(macos_crash_summary(&report(&header, &diagnostic)).is_none());
            }
        }
        let mut noncrash = body.clone();
        noncrash.as_object_mut().unwrap().remove("exception");
        noncrash["termination"] = serde_json::json!({"namespace":"EXIT","code":0});
        assert!(macos_crash_summary(&report(&header, &noncrash)).is_none());
        let mut stackshot = header.clone();
        stackshot["bug_type"] = serde_json::json!("288");
        assert!(macos_crash_summary(&report(&stackshot, &body)).is_none());
        for note in [
            "NON-FATAL CONDITION (this is NOT a crash)",
            "SIMULATED (this is NOT a crash)",
        ] {
            assert!(
                macos_crash_summary(&format!(
                    "Process: REAPER [42]\nException Type: EXC_RESOURCE\nException Note: {note}\n"
                ))
                .is_none()
            );
        }
    }

    #[test]
    fn compact_macos_artifact_keeps_frames_while_removing_personal_fields() {
        let report = r#"{"app_name":"REAPER","bug_type":"309"}
{"pid":7,"crashReporterKey":"personal-device-id","userID":501,"faultingThread":0,"threads":[{"frames":[{"symbol":"kontra_render","imageIndex":0}]}],"usedImages":[{"name":"KONTRA","path":"/Users/private/plugins/KONTRA.vst3"}],"exception":{"type":"EXC_BAD_ACCESS"}}"#;
        let full = macos_summary_for_pid(7, [report.to_owned()]).unwrap();
        assert!(full.contains("kontra_render") && full.contains("EXC_BAD_ACCESS"));
        assert!(!full.contains("personal-device-id") && !full.contains("/Users/private"));
    }
    #[test]
    fn legacy_macos_report_requires_pid_and_preserves_exception_and_frames() {
        let report = "Process: REAPER [4321]\nException Type: EXC_BAD_ACCESS (SIGSEGV)\nThread 3 Crashed:\n0 KONTRA 0x1234 render+12\n1 Metal driver+4\n";
        let (pid, summary) = macos_crash_summary(report).unwrap();
        assert_eq!(pid, 4321);
        assert!(summary.contains("render+12"));
        assert!(macos_summary_for_pid(99, [report.to_owned()]).is_none());
        assert!(
            macos_summary_for_pid(4321, [report.to_owned()])
                .unwrap()
                .contains("EXC_BAD_ACCESS")
        );
        assert!(macos_crash_summary("Process: REAPER [4321]\nNormal exit").is_none());
    }

    /// A reused pid must not pass for the host that wrote the marker, or the crash that freed
    /// the pid is never reported.
    #[test]
    fn a_live_pid_running_another_program_is_not_the_host() {
        let pid = std::process::id();
        let exe = std::env::current_exe().expect("current exe");
        let exe = exe.file_name().expect("exe name").to_string_lossy();
        assert!(process_is_alive(pid, &exe));
        assert!(!process_is_alive(pid, "some-other-daw.exe"));
    }

    #[test]
    fn macos_summary_keeps_multiline_vm_failure_evidence() {
        let report = r#"{"app_name":"KONTRA","bug_type":"309"}
{
  "pid": 873,
  "procName": "KONTRA",
  "exception": {"type":"EXC_CRASH","signal":"SIGABRT"},
  "termination": {"indicator":"Abort trap: 6"},
  "ktriageinfo": "mach_vm_allocate_kernel failed within call to vm_map_enter",
  "asi": {"libsystem_c.dylib":["abort() called"]},
  "faultingThread": 0,
  "threads": [{"queue":"com.apple.main-thread","frames":[]}],
  "vmSummary": "MALLOC 648.1M; TOTAL 2.7G"
}"#;

        let (pid, summary) = macos_crash_summary(report).expect("valid macOS crash report");
        assert_eq!(pid, 873);
        assert!(summary.contains("mach_vm_allocate_kernel failed"));
        assert!(summary.contains("MALLOC 648.1M"));
    }

    #[test]
    fn macos_newer_wrong_pid_does_not_shadow_exact_pid() {
        let wrong = "{\"app_name\":\"KONTRA\"}\n{\"pid\":999,\"procName\":\"KONTRA\"}";
        let exact = "{\"app_name\":\"KONTRA\",\"bug_type\":\"309\"}\n{\"pid\":873,\"procName\":\"KONTRA\",\"exception\":{\"type\":\"EXC_BAD_ACCESS\",\"signal\":\"SIGSEGV\"}}";

        let summary = macos_summary_for_pid(873, [wrong.to_string(), exact.to_string()])
            .expect("older exact-PID report should be selected");

        assert!(summary.contains("\"pid\": 873"));
        assert!(
            macos_crash_summary("{\"app_name\":\"KONTRA\"}\n{\"pid\":873,\"procName\":\"KONTRA\"}")
                .is_none()
        );
    }

    #[test]
    fn windows_error_reports_are_decoded_from_utf16() {
        let mut report = vec![0xff, 0xfe];
        for unit in "Version=131072\nEventType=APPCRASH\n".encode_utf16() {
            report.extend_from_slice(&unit.to_le_bytes());
        }
        // A truncated trailing code unit must not derail the rest.
        report.push(0x41);
        assert_eq!(
            decode_report_text(&report),
            "Version=131072\nEventType=APPCRASH\n"
        );
        assert_eq!(decode_report_text(b"\xef\xbb\xbfplain"), "plain");
        assert_eq!(decode_report_text(b"plain"), "plain");
    }

    #[test]
    fn linux_coredumpctl_invocation_filters_incident_window() {
        assert_eq!(
            linux_coredumpctl_args(42, 100, 200),
            ["--no-pager", "--since=@40", "--until=@320", "info", "42",]
        );
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_command_deadline_kills_a_stalled_child() {
        let started = std::time::Instant::now();
        let output = command_output_with_deadline(
            std::process::Command::new("sleep").arg("5"),
            std::time::Duration::from_millis(50),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(output.is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn evidence_command_stops_promptly_on_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        let sentinel = directory.path().join("child-started");
        let stopping = std::sync::Arc::new(AtomicBool::new(false));
        let signal = std::sync::Arc::clone(&stopping);
        let shutdown = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !sentinel.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if !sentinel.exists() {
                return None;
            }
            let stopped_at = std::time::Instant::now();
            signal.store(true, Ordering::Release);
            Some(stopped_at)
        });
        let output = command_output_with_deadline(
            std::process::Command::new("sh")
                .arg("-c")
                .arg("printf ready > \"$1\"; exec sleep 5")
                .arg("sh")
                .arg(directory.path().join("child-started")),
            std::time::Duration::from_secs(5),
            &stopping,
        )
        .unwrap();
        let stopped_at = shutdown
            .join()
            .unwrap()
            .expect("child must start before shutdown is requested");
        assert!(output.is_none());
        assert!(stopped_at.elapsed() < std::time::Duration::from_secs(1));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn evidence_command_drains_more_than_a_pipe_buffer() {
        let output = command_output_with_deadline(
            std::process::Command::new("head").args(["-c", "131072", "/dev/zero"]),
            std::time::Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .unwrap()
        .expect("large output should finish before the deadline");
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 131_072);
    }
    #[test]
    fn linux_native_evidence_requires_the_recorded_host_executable() {
        let report = "PID: 1234 (reaper)\n    Executable: /opt/REAPER/reaper\nSignal: 11 (SEGV)";
        assert!(linux_coredump_matches_host(report, "reaper"));
        assert!(
            !linux_coredump_matches_host(report, "reason"),
            "a reused PID is insufficient"
        );
        assert!(!linux_coredump_matches_host(
            "PID: 1234 (reaper)\nSignal: 11",
            "reaper"
        ));
        assert!(!linux_coredump_matches_host(report, ""));
        assert!(linux_coredump_matches_host(
            "Executable: /opt/reaper (deleted)",
            "reaper"
        ));
        assert!(!linux_coredump_matches_host(
            &format!("{report}\nExecutable: /opt/reason"),
            "reaper"
        ));
    }
}
