//! Standalone-owned Linux fatal-signal evidence; never installed by a plug-in.
use super::platform::{CrashEvidence, EvidenceDisposition};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::{io, path::PathBuf};

const SIGNALS: [i32; 5] = [
    libc::SIGSEGV,
    libc::SIGBUS,
    libc::SIGILL,
    libc::SIGFPE,
    libc::SIGABRT,
];
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
    super::crash::reports_dir()
        .join("signals")
        .join(format!("{session_id}.json"))
}

struct SignalState {
    fd: i32,
    offset: libc::off_t,
    proc_maps_fd: i32,
    maps_fd: i32,
    executable: Box<[(usize, usize)]>,
    _stack: Box<[u8]>,
}
static STATE: AtomicPtr<SignalState> = AtomicPtr::new(std::ptr::null_mut());

pub(super) fn install(
    session: &str,
    pid: u32,
    started: u64,
    host: &str,
    build: &str,
) -> io::Result<()> {
    use std::os::fd::IntoRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    if !STATE.load(Ordering::Acquire).is_null() {
        return Ok(());
    }
    if session.len() != 16 || !session.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err(io::Error::other("invalid native-capture session identity"));
    }
    let mappings =
        super::read_bounded_file(std::path::Path::new("/proc/self/maps"), 512 * 1024)?
            .ok_or_else(|| io::Error::other("loaded image mappings exceed capture budget"))?;
    let mut maps = String::new();
    let mut executable = Vec::new();
    for line in String::from_utf8_lossy(&mappings).lines() {
        let mut fields = line.split_whitespace();
        let Some(range) = fields.next() else {
            continue;
        };
        if !fields.next().is_some_and(|p| p.contains('x')) {
            continue;
        }
        let Some((lo, hi)) = range.split_once('-').and_then(|(a, b)| {
            Some((
                usize::from_str_radix(a, 16).ok()?,
                usize::from_str_radix(b, 16).ok()?,
            ))
        }) else {
            continue;
        };
        if hi <= lo {
            continue;
        }
        executable.push((lo, hi));
        maps.push_str(line);
        maps.push('\n');
    }
    if maps.len() > 64 * 1024 {
        return Err(io::Error::other(
            "executable mappings exceed capture budget",
        ));
    }
    let initial = Capture {
        schema: 1,
        session_id: session.into(),
        pid,
        started_at: started,
        host_process: host.into(),
        build_id: build.into(),
        maps,
        captured_at: 0,
        signal: 0,
        code: 0,
        fault_address: 0,
        backtrace: Vec::new(),
    };
    let bytes = serde_json::to_vec(&initial).map_err(io::Error::other)?;
    let offset = bytes
        .windows(b"\"captured_at\":".len())
        .position(|w| w == b"\"captured_at\":")
        .ok_or_else(|| io::Error::other("native capture metadata missing timestamp"))?
        as libc::off_t;
    let file = path(session);
    buffr_durable_file::publish_private_streaming(
        &file,
        std::time::Duration::from_millis(500),
        |f| std::io::Write::write_all(f, &bytes),
    )?;
    let capture_file = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(file)?;
    let proc_maps = std::fs::File::open("/proc/self/maps")?;
    let mut maps_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path(session).with_extension("maps"))?;
    std::io::Write::write_all(&mut maps_file, &mappings)?;
    let mut stack = vec![0u8; 128 * 1024].into_boxed_slice();
    let alternate = libc::stack_t {
        ss_sp: stack.as_mut_ptr().cast(),
        ss_flags: 0,
        ss_size: stack.len(),
    };
    // SAFETY: the standalone retains the file, stack and immutable state until process exit.
    unsafe {
        if libc::sigaltstack(&alternate, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
        let state = Box::into_raw(Box::new(SignalState {
            fd: capture_file.into_raw_fd(),
            offset,
            proc_maps_fd: proc_maps.into_raw_fd(),
            maps_fd: maps_file.into_raw_fd(),
            executable: executable.into_boxed_slice(),
            _stack: stack,
        }));
        STATE.store(state, Ordering::Release);
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = fatal_signal as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND | libc::SA_ONSTACK;
        libc::sigemptyset(&mut action.sa_mask);
        for signal in SIGNALS {
            if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
    }
    Ok(())
}

fn executable_address(address: usize, maps: &[u8]) -> bool {
    let hex = |bytes: &[u8]| {
        bytes.iter().try_fold(0usize, |n, b| {
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return None,
            };
            n.checked_mul(16)?.checked_add(usize::from(digit))
        })
    };
    maps.split(|b| *b == b'\n').any(|line| {
        let mut fields = line
            .split(|b| b.is_ascii_whitespace())
            .filter(|v| !v.is_empty());
        let Some(range) = fields.next() else {
            return false;
        };
        if !fields.next().is_some_and(|p| p.contains(&b'x')) {
            return false;
        }
        let Some(split) = range.iter().position(|b| *b == b'-') else {
            return false;
        };
        match (hex(&range[..split]), hex(&range[split + 1..])) {
            (Some(lo), Some(hi)) => (lo..hi).contains(&address),
            _ => false,
        }
    })
}

struct Buffer {
    bytes: [u8; 4096],
    len: usize,
}
impl Buffer {
    fn append(&mut self, value: &[u8]) {
        let n = value.len().min(self.bytes.len() - self.len);
        self.bytes[self.len..self.len + n].copy_from_slice(&value[..n]);
        self.len += n;
    }
    fn number(&mut self, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        loop {
            at -= 1;
            digits[at] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.append(&digits[at..]);
    }
}

unsafe extern "C" fn fatal_signal(
    signal: i32,
    info: *mut libc::siginfo_t,
    context: *mut libc::c_void,
) {
    let state = STATE.load(Ordering::Acquire);
    if !state.is_null() && !context.is_null() && !info.is_null() {
        // SAFETY: the kernel supplies the signal context; state remains immutable and live.
        let (state, info, context) =
            unsafe { (&*state, &*info, &*(context.cast::<libc::ucontext_t>())) };
        // GPU/audio libraries may have loaded after startup; retain their current mappings too.
        let mut maps = [0u8; 64 * 1024];
        let map_bytes =
            unsafe { libc::pread(state.proc_maps_fd, maps.as_mut_ptr().cast(), maps.len(), 0) };
        let map_bytes = if map_bytes > 0 { map_bytes as usize } else { 0 };
        if map_bytes != 0 {
            let n = unsafe { libc::pwrite(state.maps_fd, maps.as_ptr().cast(), map_bytes, 0) };
            if n == map_bytes as isize {
                unsafe {
                    libc::ftruncate(state.maps_fd, n as libc::off_t);
                }
            }
        }
        #[cfg(target_arch = "x86_64")]
        let (pc, sp, mut fp) = (
            context.uc_mcontext.gregs[libc::REG_RIP as usize] as usize,
            context.uc_mcontext.gregs[libc::REG_RSP as usize] as usize,
            context.uc_mcontext.gregs[libc::REG_RBP as usize] as usize,
        );
        #[cfg(target_arch = "aarch64")]
        let (pc, sp, mut fp) = (
            context.uc_mcontext.pc as usize,
            context.uc_mcontext.sp as usize,
            context.uc_mcontext.regs[29] as usize,
        );
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let (pc, sp, mut fp) = (0usize, 0usize, 0usize);
        let mut frames = [0u64; FRAMES];
        frames[0] = pc as u64;
        let mut count = 1;
        while count < FRAMES
            && fp >= sp
            && fp - sp < 8 * 1024 * 1024
            && fp % std::mem::size_of::<usize>() == 0
        {
            let mut pair = [0usize; 2];
            let local = libc::iovec {
                iov_base: pair.as_mut_ptr().cast(),
                iov_len: std::mem::size_of_val(&pair),
            };
            let remote = libc::iovec {
                iov_base: fp as *mut _,
                iov_len: local.iov_len,
            };
            // Kernel-checked reads fail at corrupt frame pointers instead of faulting again.
            let read = unsafe {
                libc::syscall(
                    libc::SYS_process_vm_readv,
                    libc::getpid(),
                    &local as *const libc::iovec,
                    1usize,
                    &remote as *const libc::iovec,
                    1usize,
                    0usize,
                )
            };
            let executable = if map_bytes != 0 {
                executable_address(pair[1], &maps[..map_bytes])
            } else {
                state
                    .executable
                    .iter()
                    .any(|&(lo, hi)| (lo..hi).contains(&pair[1]))
            };
            if read != local.iov_len as libc::c_long || !executable {
                break;
            }
            frames[count] = pair[1] as u64;
            count += 1;
            if pair[0] <= fp {
                break;
            }
            fp = pair[0];
        }
        let mut time: libc::timespec = unsafe { std::mem::zeroed() };
        unsafe {
            libc::clock_gettime(libc::CLOCK_REALTIME, &mut time);
        }
        let mut out = Buffer {
            bytes: [0; 4096],
            len: 0,
        };
        out.append(b"\"captured_at\":");
        out.number(time.tv_sec.max(0) as u64);
        out.append(b",\"signal\":");
        out.number(signal as u64);
        out.append(b",\"code\":");
        if info.si_code < 0 {
            out.append(b"-");
        }
        out.number(i64::from(info.si_code).unsigned_abs());
        out.append(b",\"fault_address\":");
        out.number(if info.si_code > 0 {
            unsafe { info.si_addr() as u64 }
        } else {
            0
        });
        out.append(b",\"backtrace\":[");
        for (n, frame) in frames[..count].iter().enumerate() {
            if n != 0 {
                out.append(b",");
            }
            out.number(*frame);
        }
        out.append(b"]}");
        let mut written = 0;
        for _ in 0..4 {
            let n = unsafe {
                libc::pwrite(
                    state.fd,
                    out.bytes[written..out.len].as_ptr().cast(),
                    out.len - written,
                    state.offset + written as libc::off_t,
                )
            };
            if n > 0 {
                written += n as usize;
            }
            if written == out.len {
                unsafe {
                    libc::ftruncate(state.fd, state.offset + out.len as libc::off_t);
                }
                break;
            }
        }
        if written != out.len {
            unsafe {
                libc::write(libc::STDERR_FILENO, out.bytes.as_ptr().cast(), out.len);
            }
        }
    }
    // SA_RESETHAND restores the fatal default; preserve the original signal/core exit status.
    unsafe {
        libc::raise(signal);
    }
}

pub(super) fn evidence(
    session: &str,
    pid: u32,
    started: u64,
    ended: u64,
    host: &str,
) -> Option<CrashEvidence> {
    if session.len() != 16 || !session.bytes().all(|v| v.is_ascii_hexdigit()) {
        return None;
    }
    let file = path(session);
    if !std::fs::symlink_metadata(&file).ok()?.is_file() {
        return None;
    }
    let bytes = super::read_bounded_file(&file, 128 * 1024).ok()??;
    let captured: Capture = serde_json::from_slice(&bytes).ok()?;
    if captured.schema != 1
        || captured.session_id != session
        || captured.pid != pid
        || captured.started_at != started
        || captured.host_process != host
        || !(started..=ended).contains(&captured.captured_at)
        || !SIGNALS.contains(&captured.signal)
        || captured.backtrace.is_empty()
        || captured.backtrace.len() > FRAMES
    {
        return None;
    }
    let signal = match captured.signal {
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGBUS => "SIGBUS",
        libc::SIGILL => "SIGILL",
        libc::SIGFPE => "SIGFPE",
        libc::SIGABRT => "SIGABRT",
        _ => unreachable!(),
    };
    let mut text = format!(
        "Standalone native signal {signal} ({}) · code {} · fault address {:#x}\nBuild: {}\nNative backtrace (raw instruction addresses; bounded frame-pointer walk):\n",
        captured.signal, captured.code, captured.fault_address, captured.build_id
    );
    for (index, address) in captured.backtrace.iter().enumerate() {
        use std::fmt::Write;
        let _ = writeln!(text, "#{index} {address:#x}");
    }
    // ponytail: add CFI unwinding if omitted frame pointers prevent useful multi-frame traces.
    text.push_str("Optimized/corrupt frames may stop the walk; no stack contents are captured.\n");
    let maps_file = file.with_extension("maps");
    let maps = std::fs::symlink_metadata(&maps_file)
        .ok()
        .filter(|m| m.is_file())
        .and_then(|_| {
            super::read_bounded_file(&maps_file, 512 * 1024)
                .ok()
                .flatten()
        });
    if let Some(maps) = maps {
        text.push_str("Loaded image mappings (bounded fault-time snapshot; startup fallback):\n");
        text.push_str(&String::from_utf8_lossy(&maps));
    } else {
        text.push_str("Loaded image mappings at startup:\n");
        text.push_str(&captured.maps);
    }
    Some(CrashEvidence {
        disposition: EvidenceDisposition::Crash,
        signature: format!(
            "standalone-native:{signal}:{}:{}",
            captured.code, captured.build_id
        ),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::process::ExitStatusExt, process::Command};
    const TEST: &str =
        "support::standalone::tests::standalone_fault_and_panic_are_recovered_without_coredumpctl";
    const CHILD: &str = "KONTRA_NATIVE_CAPTURE_CHILD";

    #[test]
    fn frame_addresses_require_executable_mappings() {
        let maps = b"1000-2000 rw-p 0 00:00 0 data\n3000-4000 r-xp 0 00:00 0 image\n";
        assert!(!executable_address(0x1500, maps));
        assert!(executable_address(0x3000, maps));
        assert!(executable_address(0x3fff, maps));
        assert!(!executable_address(0x4000, maps));
        assert!(!executable_address(1, b"invalid-range r-xp"));
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(never)]
    unsafe fn authored_fault(segv: bool) -> ! {
        if segv {
            unsafe {
                std::arch::asm!("xor rax, rax", "mov byte ptr [rax], 0", options(noreturn));
            }
        }
        unsafe {
            std::arch::asm!("ud2", options(noreturn));
        }
    }

    #[test]
    fn standalone_fault_and_panic_are_recovered_without_coredumpctl() {
        if let Some(kind) = std::env::var_os(CHILD) {
            if kind == "recover" {
                let incident = super::super::crash::detect_stale_sessions(
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
                let value = serde_json::to_value(&incident).unwrap();
                let expected = std::env::var("KONTRA_CAPTURE_EXPECT").unwrap();
                assert_eq!(value["kind"], expected);
                if expected == "platform_crash" {
                    let delivered = super::super::crash::render_complete_diagnostics(&incident);
                    assert!(delivered.contains(&std::env::var("KONTRA_CAPTURE_SIGNAL").unwrap()));
                    assert!(delivered.contains("Native backtrace"));
                    let session =
                        std::fs::read_dir(super::super::crash::reports_dir().join("signals"))
                            .unwrap()
                            .flatten()
                            .find(|e| e.path().extension().is_some_and(|x| x == "json"))
                            .unwrap();
                    let capture: Capture =
                        serde_json::from_slice(&std::fs::read(session.path()).unwrap()).unwrap();
                    let ended = capture.captured_at;
                    assert!(
                        evidence(
                            &capture.session_id,
                            capture.pid,
                            capture.started_at,
                            ended,
                            &capture.host_process
                        )
                        .is_some()
                    );
                    assert!(
                        evidence(
                            &capture.session_id,
                            capture.pid + 1,
                            capture.started_at,
                            ended,
                            &capture.host_process
                        )
                        .is_none()
                    );
                    assert!(
                        evidence(
                            &capture.session_id,
                            capture.pid,
                            capture.started_at,
                            ended,
                            "wrong-host"
                        )
                        .is_none()
                    );
                    assert!(
                        evidence(
                            &capture.session_id,
                            capture.pid,
                            capture.started_at + 1,
                            ended,
                            &capture.host_process
                        )
                        .is_none()
                    );
                    let destination = std::env::var_os("KONTRA_CAPTURE_EXPORT").unwrap();
                    let exported = super::super::export_crash_evidence(
                        std::path::Path::new(&destination),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap();
                    assert!(exported.warnings.is_empty(), "{:?}", exported.warnings);
                }
                return;
            }
            let _session = crate::support::start_standalone_session();
            // Keep synthetic faults out of the machine's native core store.
            unsafe {
                libc::setrlimit(
                    libc::RLIMIT_CORE,
                    &libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    },
                );
            }
            if kind == "panic" {
                let _ = std::panic::catch_unwind(|| panic!("authored standalone panic"));
                std::process::exit(101);
            }
            if kind == "kill" {
                unsafe {
                    libc::raise(libc::SIGKILL);
                }
            }
            #[cfg(target_arch = "x86_64")]
            {
                let address = authored_fault as *const () as usize;
                std::fs::write(
                    std::path::PathBuf::from(std::env::var_os("KONTRA_REPORT_DIR").unwrap())
                        .join("fault-address"),
                    address.to_string(),
                )
                .unwrap();
                unsafe {
                    authored_fault(kind == "segv");
                }
            }
            #[cfg(not(target_arch = "x86_64"))]
            unsafe {
                libc::raise(if kind == "segv" {
                    libc::SIGSEGV
                } else {
                    libc::SIGILL
                });
            }
            unreachable!();
        }
        for kind in ["signal", "segv", "panic", "kill"] {
            let directory = tempfile::tempdir().unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .env(CHILD, kind)
                .env("KONTRA_REPORT_DIR", directory.path())
                .env("PATH", "")
                .args(["--exact", TEST, "--test-threads=1"])
                .output()
                .unwrap();
            if kind == "signal" {
                assert_eq!(output.status.signal(), Some(libc::SIGILL));
            }
            if kind == "segv" {
                assert_eq!(output.status.signal(), Some(libc::SIGSEGV));
            }
            if kind == "kill" {
                assert_eq!(output.status.signal(), Some(libc::SIGKILL));
            }
            let report = directory.path().join("crash-reports");
            let marker = std::fs::read_dir(report.join("sessions"))
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .map(|e| {
                    serde_json::from_slice::<serde_json::Value>(&std::fs::read(e.path()).unwrap())
                        .unwrap()
                })
                .next()
                .unwrap();
            let session = marker["session_id"].as_str().unwrap();
            let pid = marker["pid"].as_u64().unwrap() as u32;
            let started = marker["started_at"].as_u64().unwrap();
            let host = marker["host_process"].as_str().unwrap();
            // Recovery runs with the isolated directory only in a separate child.
            let raw = report.join("signals").join(format!("{session}.json"));
            if kind == "signal" || kind == "segv" {
                let capture: Capture = serde_json::from_slice(
                    &std::fs::read(raw).expect("native signal evidence absent"),
                )
                .unwrap();
                assert_eq!(capture.pid, pid);
                assert_eq!(capture.started_at, started);
                assert_eq!(capture.host_process, host);
                assert_eq!(
                    capture.signal,
                    if kind == "segv" {
                        libc::SIGSEGV
                    } else {
                        libc::SIGILL
                    }
                );
                assert!(!capture.backtrace.is_empty());
                assert_ne!(capture.backtrace[0], 0);
                assert!(capture.backtrace.len() <= FRAMES);
                assert!(capture.captured_at >= started);
                assert!(!capture.maps.is_empty());
                #[cfg(target_arch = "x86_64")]
                {
                    let address: u64 =
                        std::fs::read_to_string(directory.path().join("fault-address"))
                            .unwrap()
                            .parse()
                            .unwrap();
                    assert!(
                        (address..address + 128).contains(&capture.backtrace[0]),
                        "top frame must be the actual fault instruction"
                    );
                }
            } else if kind == "panic" {
                let panic: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(report.join("panics").join(format!("{pid}.json")))
                        .expect("standalone panic evidence absent"),
                )
                .unwrap();
                assert!(
                    panic["message"]
                        .as_str()
                        .unwrap()
                        .contains("authored standalone panic")
                );
                let trace = panic["message"].as_str().unwrap();
                assert!(trace.contains("Rust backtrace:"));
                assert!(
                    trace
                        .lines()
                        .any(|line| line.trim_start().starts_with("0:")),
                    "captured panic must include frames"
                );
            } else if let Ok(bytes) = std::fs::read(raw) {
                let capture: Capture = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    capture.signal, 0,
                    "SIGKILL must not be invented as a captured fault"
                );
            }
            let export = tempfile::tempdir().unwrap();
            let expected = match kind {
                "signal" | "segv" => "platform_crash",
                "panic" => "panic",
                _ => "unclean_exit",
            };
            let recovered = Command::new(std::env::current_exe().unwrap())
                .env(CHILD, "recover")
                .env("KONTRA_CAPTURE_EXPECT", expected)
                .env(
                    "KONTRA_CAPTURE_SIGNAL",
                    if kind == "segv" { "SIGSEGV" } else { "SIGILL" },
                )
                .env("KONTRA_CAPTURE_EXPORT", export.path())
                .env("KONTRA_REPORT_DIR", directory.path())
                .env("PATH", "")
                .args(["--exact", TEST, "--test-threads=1"])
                .output()
                .unwrap();
            assert!(
                recovered.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&recovered.stdout),
                String::from_utf8_lossy(&recovered.stderr)
            );
            if kind == "signal" || kind == "segv" {
                let exported = export
                    .path()
                    .join("crash-evidence/crash-reports/signals")
                    .join(format!("{session}.json"));
                assert_eq!(
                    std::fs::read(exported).unwrap(),
                    std::fs::read(report.join("signals").join(format!("{session}.json"))).unwrap()
                );
                let mappings = export
                    .path()
                    .join("crash-evidence/crash-reports/signals")
                    .join(format!("{session}.maps"));
                assert_eq!(
                    std::fs::read(mappings).unwrap(),
                    std::fs::read(report.join("signals").join(format!("{session}.maps"))).unwrap()
                );
            }
        }
    }
}
