//! Host-window helpers for an editor embedded in a DAW's parent window.
//!
//! Copied from truce-gui-utils 6.3.0 (`should_skip_frame`,
//! `reanchor_to_superview_top`; MIT/Apache-2.0, the truce authors) so this
//! crate does not depend on it. The bodies are verbatim; only lint
//! attributes and SAFETY comments were added, and the handles moved to
//! raw-window-handle 0.6, whose pointers are never null. No-ops off
//! macOS/Windows.
use raw_window_handle::RawWindowHandle;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsRect {
    origin: NsPoint,
    size: NsSize,
}

/// Re-anchor the editor's `NSView` to the **top** of its superview in
/// unflipped Cocoa coordinates: resizing the child leaves its origin alone,
/// so a taller child would grow *down* off the parent's top. Call each
/// frame. No-op on non-macOS.
#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "Objective-C messages to the host's views")]
#[expect(
    unexpected_cfgs,
    reason = "objc 0.2's msg_send! tests the retired `cargo-clippy` feature"
)]
pub fn reanchor_to_superview_top(handle: RawWindowHandle) {
    use objc::{msg_send, sel, sel_impl};

    let view_ptr = match handle {
        RawWindowHandle::AppKit(h) => h.ns_view.as_ptr(),
        _ => return,
    };

    // SAFETY: `view_ptr` is baseview's live, non-null NSView for this
    // window; `superview`, `frame` and `setFrameOrigin:` are plain AppKit
    // messages on the main thread that owns it.
    unsafe {
        let view = view_ptr.cast::<objc::runtime::Object>();
        let superview: *mut objc::runtime::Object = msg_send![view, superview];
        if superview.is_null() {
            return;
        }
        let parent_frame: NsRect = msg_send![superview, frame];
        let child_frame: NsRect = msg_send![view, frame];
        let new_y = parent_frame.size.height - child_frame.size.height;
        if (new_y - child_frame.origin.y).abs() < f64::EPSILON {
            return;
        }
        let new_origin = NsPoint {
            x: child_frame.origin.x,
            y: new_y,
        };
        let _: () = msg_send![view, setFrameOrigin: new_origin];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reanchor_to_superview_top(_handle: RawWindowHandle) {}

/// Whether this tick's frame should be skipped: the editor's view is
/// detached from any window, or the host window is not visible (macOS
/// occlusion; Windows hidden or minimized). A hidden window cannot present,
/// and on Windows `on_frame` runs on the host's GUI thread. Always `false`
/// elsewhere.
#[cfg(target_os = "macos")]
#[must_use]
#[expect(unsafe_code, reason = "Objective-C messages to the host's views")]
#[expect(
    unexpected_cfgs,
    reason = "objc 0.2's msg_send! tests the retired `cargo-clippy` feature"
)]
pub fn should_skip_frame(handle: RawWindowHandle) -> bool {
    use objc::{msg_send, sel, sel_impl};

    let view_ptr = match handle {
        RawWindowHandle::AppKit(h) => h.ns_view.as_ptr(),
        _ => return false,
    };

    // SAFETY: `view_ptr` is baseview's live, non-null NSView; `window` and
    // `occlusionState` are plain AppKit queries on the thread that owns it.
    unsafe {
        let view = view_ptr.cast::<objc::runtime::Object>();
        let window: *mut objc::runtime::Object = msg_send![view, window];
        if window.is_null() {
            // Detached from any window - nothing to present into.
            return true;
        }
        // `NSWindowOcclusionStateVisible` == 1 << 1. Bit clear => the
        // window is not visible (minimized or fully covered).
        let state: u64 = msg_send![window, occlusionState];
        state & (1 << 1) == 0
    }
}

#[cfg(target_os = "windows")]
#[must_use]
#[expect(unsafe_code, reason = "two Win32 window-state queries")]
pub fn should_skip_frame(handle: RawWindowHandle) -> bool {
    unsafe extern "system" {
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
        fn IsIconic(hwnd: *mut std::ffi::c_void) -> i32;
    }

    let hwnd = match handle {
        RawWindowHandle::Win32(h) => h.hwnd.get() as *mut std::ffi::c_void,
        _ => return false,
    };
    // SAFETY: both are pure state queries on a window handle baseview
    // owns for the editor's lifetime; no aliasing or threading concerns,
    // and they're called from the GUI thread that owns the HWND.
    unsafe { IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[must_use]
pub fn should_skip_frame(_handle: RawWindowHandle) -> bool {
    false
}
