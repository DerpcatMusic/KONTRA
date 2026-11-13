//! Gives the allocator's free memory back to the system after an instrument
//! is unloaded: glibc keeps what a dropped engine freed, so resident memory
//! would stay at the sum of every instrument ever loaded.

/// Returns freed heap pages to the OS. Control thread only (it takes the
/// allocator's locks and can take milliseconds). A no-op off Linux glibc.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[expect(unsafe_code, reason = "glibc's malloc_trim has no safe wrapper")]
pub(super) fn release_freed_memory() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: malloc_trim takes no pointers and is safe to call from any
    // thread at any time.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub(super) fn release_freed_memory() {}
