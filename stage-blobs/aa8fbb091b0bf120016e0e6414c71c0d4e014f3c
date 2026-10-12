//! Host-window helpers for an editor embedded in a DAW's parent window.
//!
//! Copied from truce-gui-utils 6.3.0 (`should_skip_frame`,
//! `reanchor_to_superview_top`; MIT/Apache-2.0, the truce authors) so this
//! crate does not depend on it. The bodies are verbatim; only lint
//! attributes and SAFETY comments were added, and the handles moved to
//! raw-window-handle 0.6, whose pointers are never null. No-ops off
//! macOS/Windows.
use raw_window_handle::RawWindowHandle;

/// AppKit requires the process main thread, not merely a named UI thread.
#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "pure libSystem query of the current thread")]
pub fn main_thread_status() -> Option<bool> {
    unsafe extern "C" { fn pthread_main_np() -> std::ffi::c_int; }
    // SAFETY: pthread_main_np takes no arguments and only reads thread state.
    Some(unsafe { pthread_main_np() } != 0)
}

#[cfg(not(target_os = "macos"))]
pub fn main_thread_status() -> Option<bool> { None }


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

/// KONTAKTO patch: put the pointer at `x`, `y` physical pixels from the top
/// left of `window`'s view, where a drag that hid it began. X11 (through
/// Xlib, loaded at run time as baseview loads it), Windows and macOS; a
/// no-op anywhere else, and wherever the handles are not those.
#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "two Xlib calls on baseview's live display")]
pub fn warp_pointer(window: &baseview::WindowContext, x: f64, y: f64, _scale: f64) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle};
    let target = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => h.window,
        Ok(RawWindowHandle::Xcb(h)) => std::os::raw::c_ulong::from(h.window.get()),
        _ => return,
    };
    let Ok(RawDisplayHandle::Xlib(d)) = window.display_handle().map(|h| h.as_raw()) else {
        return;
    };
    let Some(display) = d.display else { return };
    // ponytail: loads libX11 on each call; it runs once per drag let go.
    let Ok(xlib) = x11_dl::xlib::Xlib::open() else { return };
    // SAFETY: `display` is baseview's open Xlib display and `target` its
    // window, both alive while the handler that calls this is; XWarpPointer
    // and XFlush only queue and send a request on it, on its own thread.
    unsafe {
        let display = display.as_ptr().cast();
        (xlib.XWarpPointer)(display, 0, target, 0, 0, 0, 0, x.round() as i32, y.round() as i32);
        (xlib.XFlush)(display);
    }
}

#[cfg(target_os = "windows")]
#[expect(unsafe_code, reason = "two Win32 cursor calls")]
pub fn warp_pointer(window: &baseview::WindowContext, x: f64, y: f64, _scale: f64) {
    use raw_window_handle::HasWindowHandle;
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    unsafe extern "system" {
        fn ClientToScreen(hwnd: *mut std::ffi::c_void, point: *mut Point) -> i32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
    }
    let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else {
        return;
    };
    let mut at = Point {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    // SAFETY: the HWND is baseview's, alive for the handler's lifetime;
    // `at` is a live POINT. Both calls run on the window's own thread.
    unsafe {
        if ClientToScreen(h.hwnd.get() as *mut std::ffi::c_void, &mut at) != 0 {
            SetCursorPos(at.x, at.y);
        }
    }
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "AppKit conversions and a Quartz warp")]
#[expect(
    unexpected_cfgs,
    reason = "objc 0.2's msg_send! tests the retired `cargo-clippy` feature"
)]
pub fn warp_pointer(window: &baseview::WindowContext, x: f64, y: f64, scale: f64) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::HasWindowHandle;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWarpMouseCursorPosition(point: NsPoint) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
    }
    let Ok(RawWindowHandle::AppKit(h)) = window.window_handle().map(|h| h.as_raw()) else {
        return;
    };
    // SAFETY: `ns_view` is baseview's live NSView; these are AppKit
    // geometry queries on the main thread that owns it, and the warp takes
    // a plain point.
    unsafe {
        let view = h.ns_view.as_ptr().cast::<Object>();
        let ns_window: *mut Object = msg_send![view, window];
        let screens: *mut Object = msg_send![class!(NSScreen), screens];
        if ns_window.is_null() || screens.is_null() {
            return;
        }
        let main: *mut Object = msg_send![screens, firstObject];
        if main.is_null() {
            return;
        }
        // Points in the (flipped) view, to the window, to the screen, whose
        // origin is bottom left; Quartz counts from the main screen's top.
        let local = NsPoint { x: x / scale, y: y / scale };
        let nil: *mut Object = std::ptr::null_mut();
        let in_window: NsPoint = msg_send![view, convertPoint: local toView: nil];
        let on_screen: NsPoint = msg_send![ns_window, convertPointToScreen: in_window];
        let frame: NsRect = msg_send![main, frame];
        CGWarpMouseCursorPosition(NsPoint { x: on_screen.x, y: frame.size.height - on_screen.y });
        // A warp holds the pointer still a moment unless reconnected.
        CGAssociateMouseAndMouseCursorPosition(1);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn warp_pointer(_window: &baseview::WindowContext, _x: f64, _y: f64, _scale: f64) {}
