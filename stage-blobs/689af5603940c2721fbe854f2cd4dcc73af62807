#![expect(clippy::unwrap_used, reason = "To be refactored later")]

use std::{
    collections::HashMap,
    ffi::c_int,
    ptr,
    sync::{LazyLock, RwLock},
};

use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        CallNextHookEx, GetParent, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK, MSG,
        PM_REMOVE, WH_GETMESSAGE, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_SYSCHAR, WM_SYSKEYDOWN,
        WM_SYSKEYUP, WM_USER,
    },
};

use super::*;
use crate::wrappers::win32::window::wnd_proc;

// track all windows opened by this instance of baseview
// we use an RwLock here since the vast majority of uses (event interceptions)
// will only need to read from the HashSet
static HOOK_STATE: LazyLock<RwLock<KeyboardHookState>> = LazyLock::new(RwLock::default);

pub(crate) struct KeyboardHookHandle(HWNDWrapper);

#[derive(Default)]
struct KeyboardHookState {
    hook: Option<HHOOK>,
    open_windows: HashMap<HWNDWrapper, KeyboardOwnership>,
}

/// MOOSE (KURV K23): per-window keyboard ownership.
///
/// Upstream always steals every key addressed to a baseview window from the host, so DAW
/// shortcuts and the host's typing keyboard stop working while the editor has focus. With
/// `capture` off, key messages are retargeted to the parent HWND and stay in the host's own
/// message pump (with their scan code, repeat count and layout intact).
struct KeyboardOwnership {
    capture: bool,
    /// Whether the key-down for a scan code was captured, so its repeats and its key-up go
    /// to the same owner even if `capture` changes while the key is held.
    held: [Option<bool>; 512],
}

impl KeyboardOwnership {
    fn new() -> Self {
        // Upstream behaviour by default: capture everything.
        Self { capture: true, held: [None; 512] }
    }
}

pub(crate) fn set_keyboard_capture(hwnd: HWND, capture: bool) {
    let mut state = HOOK_STATE.write().unwrap_or_else(|e| e.into_inner());
    if let Some(owner) = state.open_windows.get_mut(&HWNDWrapper(hwnd)) {
        owner.capture = capture;
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct HWNDWrapper(HWND);

// SAFETY: it's a pointer behind an RwLock. we'll live
unsafe impl Send for KeyboardHookState {}
unsafe impl Sync for KeyboardHookState {}

// SAFETY: we never access the underlying HWND ourselves, just use it as a HashSet entry
unsafe impl Send for HWNDWrapper {}
unsafe impl Sync for HWNDWrapper {}

impl Drop for KeyboardHookHandle {
    fn drop(&mut self) {
        deinit_keyboard_hook(self.0);
    }
}

// initialize keyboard hook
// some DAWs (particularly Ableton) intercept incoming keyboard messages,
// but we're naughty so we intercept them right back (while the window wants the keyboard)
pub(crate) fn init_keyboard_hook(hwnd: HWND) -> KeyboardHookHandle {
    let state = &mut *HOOK_STATE.write().unwrap();

    // register hwnd to global window set
    state.open_windows.insert(HWNDWrapper(hwnd), KeyboardOwnership::new());

    if state.hook.is_some() {
        // keyboard hook already exists, just return handle
        KeyboardHookHandle(HWNDWrapper(hwnd))
    } else {
        // keyboard hook doesn't exist (no windows open before this), create it
        let new_hook = unsafe {
            SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(keyboard_hook_callback),
                GetModuleHandleW(ptr::null()),
                GetCurrentThreadId(),
            )
        };

        state.hook = Some(new_hook);

        KeyboardHookHandle(HWNDWrapper(hwnd))
    }
}

fn deinit_keyboard_hook(hwnd: HWNDWrapper) {
    let state = &mut *HOOK_STATE.write().unwrap();

    state.open_windows.remove(&hwnd);

    if state.open_windows.is_empty() {
        if let Some(hhook) = state.hook {
            unsafe {
                UnhookWindowsHookEx(hhook);
            }

            state.hook = None;
        }
    }
}

unsafe extern "system" fn keyboard_hook_callback(
    n_code: c_int, wparam: WPARAM, lparam: LPARAM,
) -> isize {
    let msg = lparam as *mut MSG;

    if n_code == HC_ACTION as i32 && wparam == PM_REMOVE as usize && offer_message_to_baseview(msg)
    {
        *msg = MSG {
            hwnd: ptr::null_mut(),
            message: WM_USER,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        };

        0
    } else {
        CallNextHookEx(ptr::null_mut(), n_code, wparam, lparam)
    }
}

// check if `msg` is a keyboard message addressed to a window
// in KeyboardHookState::open_windows, and intercept it if so
unsafe fn offer_message_to_baseview(msg: *mut MSG) -> bool {
    let msg = &mut *msg;

    // if this isn't a keyboard message, ignore it
    match msg.message {
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR => {}

        _ => return false,
    }

    // Scan code plus the extended-key bit: 9 bits, fits `held`.
    let scan = (msg.lParam as usize >> 16) & 0x1ff;
    let target = HWNDWrapper(msg.hwnd);

    let mut state = HOOK_STATE.write().unwrap_or_else(|e| e.into_inner());
    if !state.open_windows.contains_key(&target) {
        // A key-up can land on the host window when focus moved there while the key was
        // held (e.g. after `set_keyboard_capture(false)`). Forget the held state so the
        // next press of that key is routed fresh, and let the host have the message.
        if matches!(msg.message, WM_KEYUP | WM_SYSKEYUP) {
            for (window, owner) in state.open_windows.iter_mut() {
                if GetParent(window.0) == msg.hwnd {
                    if let Some(held) = owner.held.get_mut(scan) {
                        *held = None;
                    }
                }
            }
        }
        return false;
    }
    let Some(owner) = state.open_windows.get_mut(&target) else { return false };

    let default = owner.capture;
    let Some(held) = owner.held.get_mut(scan) else { return false };
    let capture = match msg.message {
        WM_KEYDOWN | WM_SYSKEYDOWN => *held.get_or_insert(default),
        WM_KEYUP | WM_SYSKEYUP => held.take().unwrap_or(default),
        _ => held.unwrap_or(default),
    };
    // wnd_proc may change focus or close the window, which takes this lock again.
    drop(state);

    if !capture {
        let parent = GetParent(msg.hwnd);
        if !parent.is_null() {
            // Leave the real message in the host's pump, addressed to the host window.
            msg.hwnd = parent;
            return false;
        }
    }

    let _ = wnd_proc::<BaseviewWindow>(msg.hwnd, msg.message, msg.wParam, msg.lParam);

    true
}
