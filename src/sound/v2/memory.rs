//! Loader-side physical memory snapshots. Never called from audio processing.

#[cfg(target_os = "linux")]
pub(super) fn available() -> Option<(usize, usize)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| {
        let line = info.lines().find(|line| line.starts_with(name))?;
        let kib = line[name.len()..]
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse::<usize>()
            .ok()?;
        kib.checked_mul(1024)
    };
    Some((field("MemAvailable:")?, field("MemTotal:")?))
}

#[cfg(target_os = "windows")]
pub(super) fn available() -> Option<(usize, usize)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status is writable, initialized and carries the API's required size.
    // https://learn.microsoft.com/windows/win32/api/sysinfoapi/ns-sysinfoapi-memorystatusex
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return None;
    }
    Some((
        usize::try_from(status.ullAvailPhys).ok()?,
        usize::try_from(status.ullTotalPhys).ok()?,
    ))
}

#[cfg(target_os = "macos")]
#[allow(
    deprecated,
    reason = "existing libc Mach bindings; no additional dependency"
)]
pub(super) fn available() -> Option<(usize, usize)> {
    unsafe extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            port: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    let mut total = 0_u64;
    let mut size = std::mem::size_of_val(&total);
    // SAFETY: name is NUL-terminated; output and its byte length match u64.
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut total as *mut u64).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || size != std::mem::size_of_val(&total) {
        return None;
    }
    // SAFETY: sysconf has no pointer arguments. All vm_statistics64 fields are integers.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return None;
    }
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: the send right is owned until deallocation below; stats/count
    // describe writable storage of the declared Mach flavor and capacity.
    let host = unsafe { libc::mach_host_self() };
    let result = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    // SAFETY: release exactly this call's send right in the current task.
    unsafe {
        mach_port_deallocate(libc::mach_task_self_, host);
    }
    if result != libc::KERN_SUCCESS || count < 4 {
        return None;
    }
    // Conservative reclaimable pages. Do not add speculative pages: Apple's
    // vm_statistics.h specifies they are already included in free_count.
    let bytes = (u64::from(stats.free_count) + u64::from(stats.inactive_count))
        .checked_mul(page as u64)?
        .min(total);
    Some((usize::try_from(bytes).ok()?, usize::try_from(total).ok()?))
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub(super) fn available() -> Option<(usize, usize)> {
    None
}
