//! MOOSE addition (KURV K24/K27): keep the plug-in image mapped for detached work.

/// Permanently pins the executable image (DLL / dylib / shared object) that contains
/// baseview, so that a thread which outlives the plug-in instance (for example a render
/// thread wedged in a graphics driver and detached after a bounded join) can never resume
/// into unloaded code.
///
/// The pin is process-lifetime and idempotent. Returns `false` when the image could not be
/// pinned: callers must then fall back to a synchronous join instead of detaching.
pub fn pin_current_image_for_detached_work() -> bool {
    static PINNED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PINNED.get_or_init(pin_image)
}

#[cfg(target_os = "windows")]
fn pin_image() -> bool {
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
    };

    let mut module = std::ptr::null_mut();
    let address = pin_image as *const () as *const u16;
    // SAFETY: with FROM_ADDRESS, `address` is an address inside this module rather than a
    // string. PIN keeps the module loaded until the process exits.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            address,
            &mut module,
        ) != 0
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pin_image() -> bool {
    // RTLD_NODELETE is not exported by `libc` on every target.
    #[cfg(target_os = "linux")]
    const RTLD_NODELETE: libc::c_int = 0x1000;
    #[cfg(target_os = "macos")]
    const RTLD_NODELETE: libc::c_int = 0x80;

    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    let symbol = pin_image as *const () as *const libc::c_void;
    // SAFETY: `info` is writable and `symbol` is an address inside this image.
    if unsafe { libc::dladdr(symbol, info.as_mut_ptr()) } == 0 {
        return false;
    }
    // SAFETY: a successful dladdr initialized `info`.
    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return false;
    }
    // SAFETY: `dli_fname` is owned by the loader and valid for this call. The handle is
    // leaked on purpose: NODELETE must stay in effect for the process lifetime.
    !unsafe { libc::dlopen(info.dli_fname, libc::RTLD_NOW | libc::RTLD_LOCAL | RTLD_NODELETE) }
        .is_null()
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn pin_image() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn pinning_is_idempotent() {
        // A PIE test executable cannot be dlopen()ed on Linux, so only the stable result
        // is checked here; pinning a real plug-in library is covered by hand in hosts.
        let first = super::pin_current_image_for_detached_work();
        assert_eq!(first, super::pin_current_image_for_detached_work());
    }
}
