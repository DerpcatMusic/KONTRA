//! Standalone-owned Linux fatal-signal evidence; never installed by a plug-in.
use super::platform::{CrashEvidence, EvidenceDisposition};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};

const SIGNALS: [i32; 5] = [libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGABRT];
const FRAMES: usize = 64;

#[derive(Serialize, Deserialize)]
struct Capture {
    schema: u8,
    session_id: String,
    pid: u32,
    started_at: u64,
    host_process: String,
    build_id: String,
    maps: String,
    captured_at: u64,
    signal: i32,
    code: i32,
    fault_address: u64,
    backtrace: Vec<u64>,
}

fn path(session_id: &str) -> PathBuf {
    super::crash::reports_dir().join("signals").join(format!("{session_id}.json"))
}

pub(super) fn install(_session: &str, _pid: u32, _started: u64, _host: &str, _build: &str) -> io::Result<()> {
    Ok(())
}

pub(super) fn evidence(session: &str, pid: u32, started: u64, ended: u64, host: &str) -> Option<CrashEvidence> {
    if session.len() != 16 || !session.bytes().all(|v| v.is_ascii_hexdigit()) { return None; }
    let file = path(session);
    if !std::fs::symlink_metadata(&file).ok()?.is_file() { return None; }
    let bytes = super::read_bounded_file(&file, 128 * 1024).ok()??;
    let captured: Capture = serde_json::from_slice(&bytes).ok()?;
    if captured.schema != 1 || captured.session_id != session || captured.pid != pid
        || captured.started_at != started || captured.host_process != host
        || !(started..=ended).contains(&captured.captured_at)
        || !SIGNALS.contains(&captured.signal) || captured.backtrace.is_empty()
        || captured.backtrace.len() > FRAMES || captured.backtrace[0] == 0 {
        return None;
    }
    let signal = match captured.signal {
        libc::SIGSEGV => "SIGSEGV", libc::SIGBUS => "SIGBUS", libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE", libc::SIGABRT => "SIGABRT", _ => unreachable!(),
    };
    let mut text = format!("Standalone native signal {signal} ({}) · code {} · fault address {:#x}\nBuild: {}\nNative backtrace (raw instruction addresses; bounded frame-pointer walk):\n",
        captured.signal, captured.code, captured.fault_address, captured.build_id);
    for (index, address) in captured.backtrace.iter().enumerate() {
        use std::fmt::Write;
        let _ = writeln!(text, "#{index} {address:#x}");
    }
    text.push_str("Optimized/corrupt frames may stop the walk; no stack contents are captured.\nLoaded image mappings at startup:\n");
    text.push_str(&captured.maps);
    Some(CrashEvidence { disposition: EvidenceDisposition::Crash,
        signature: format!("standalone-native:{signal}:{}:{}", captured.code, captured.build_id), text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{process::Command, os::unix::process::ExitStatusExt};
    const TEST: &str = "support::standalone::tests::standalone_fault_and_panic_are_recovered_without_coredumpctl";
    const CHILD: &str = "KONTRA_NATIVE_CAPTURE_CHILD";

    #[test]
    fn standalone_fault_and_panic_are_recovered_without_coredumpctl() {
        if let Some(kind) = std::env::var_os(CHILD) {
            let _session = crate::support::start_standalone_session();
            // Keep synthetic faults out of the machine's native core store.
            unsafe { libc::setrlimit(libc::RLIMIT_CORE, &libc::rlimit { rlim_cur:0, rlim_max:0 }); }
            if kind == "panic" {
                let _ = std::panic::catch_unwind(|| panic!("authored standalone panic"));
                std::process::exit(101);
            }
            if kind == "kill" { unsafe { libc::raise(libc::SIGKILL); } }
            #[cfg(target_arch = "x86_64")]
            unsafe { std::arch::asm!("ud2", options(noreturn)); }
            #[cfg(not(target_arch = "x86_64"))]
            unsafe { libc::raise(libc::SIGILL); }
            unreachable!();
        }
        for kind in ["signal", "panic", "kill"] {
            let directory = tempfile::tempdir().unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .env(CHILD, kind).env("KONTRA_REPORT_DIR", directory.path()).env("PATH", "")
                .args(["--exact", TEST, "--test-threads=1"]).output().unwrap();
            if kind == "signal" { assert_eq!(output.status.signal(), Some(libc::SIGILL)); }
            if kind == "kill" { assert_eq!(output.status.signal(), Some(libc::SIGKILL)); }
            let report = directory.path().join("crash-reports");
            let marker = std::fs::read_dir(report.join("sessions")).unwrap().flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .map(|e| serde_json::from_slice::<serde_json::Value>(&std::fs::read(e.path()).unwrap()).unwrap())
                .next().unwrap();
            let session = marker["session_id"].as_str().unwrap();
            let pid = marker["pid"].as_u64().unwrap() as u32;
            let started = marker["started_at"].as_u64().unwrap();
            let host = marker["host_process"].as_str().unwrap();
            // Recovery runs with the isolated directory only in a separate child.
            let raw = report.join("signals").join(format!("{session}.json"));
            if kind == "signal" {
                let capture: Capture = serde_json::from_slice(&std::fs::read(raw).expect("native signal evidence absent")).unwrap();
                assert_eq!(capture.pid, pid); assert_eq!(capture.started_at, started);
                assert_eq!(capture.host_process, host); assert_eq!(capture.signal, libc::SIGILL);
                assert!(!capture.backtrace.is_empty()); assert_ne!(capture.backtrace[0], 0);
                assert!(capture.backtrace.len() <= FRAMES);
                assert!(capture.captured_at >= started);
                assert!(!capture.maps.is_empty());
            } else if kind == "panic" {
                let panic: serde_json::Value = serde_json::from_slice(&std::fs::read(report.join("panics").join(format!("{pid}.json"))).expect("standalone panic evidence absent")).unwrap();
                assert!(panic["message"].as_str().unwrap().contains("authored standalone panic"));
                assert!(panic["message"].as_str().unwrap().contains("standalone_fault_and_panic"));
            } else if let Ok(bytes) = std::fs::read(raw) {
                let capture: Capture = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(capture.signal, 0, "SIGKILL must not be invented as a captured fault");
            }
        }
    }
}
