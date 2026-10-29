//! A strided interrupt. Luau calls its interrupt at every safepoint (loop back
//! edge, call, return), and mlua runs each call through its whole callback
//! wrapper: 14% of AO Strings Pizzicato 2's load. This hook counts safepoints
//! and enters mlua's (and so the runaway check) once every [`STRIDE`].

// An FFI module (09-BUILD.md §1.8): each unsafe block carries its SAFETY note.
#![allow(unsafe_code)]

use std::cell::Cell;
use std::ffi::c_int;
use std::sync::OnceLock;

use mlua::{Lua, ffi};

/// Safepoints per call of mlua's interrupt; its callback counts this many.
pub(crate) const STRIDE: u32 = 64;

type Hook = unsafe extern "C-unwind" fn(*mut ffi::lua_State, c_int);

/// mlua's interrupt for the one callback type `state` installs.
static MLUA: OnceLock<Hook> = OnceLock::new();

thread_local! {
    static TICKS: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "C-unwind" fn strided(state: *mut ffi::lua_State, gc: c_int) {
    let due = TICKS.with(|t| {
        let n = t.get().wrapping_add(1);
        t.set(n);
        n % STRIDE == 0
    });
    if due && let Some(hook) = MLUA.get() {
        // SAFETY: mlua's own interrupt, called as Luau would call it.
        unsafe { hook(state, gc) };
    }
}

/// Puts [`strided`] in front of the interrupt `set_interrupt` installed. A
/// hook other than the first one seen is left as it is.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    // SAFETY: `lua_callbacks` is the state's own callback table, read and
    // written on the thread that owns the state while mlua holds its lock.
    unsafe {
        lua.exec_raw::<()>((), |state| {
            let callbacks = ffi::lua_callbacks(state);
            if let Some(hook) = (*callbacks).interrupt
                && *MLUA.get_or_init(|| hook) as usize == hook as usize
            {
                (*callbacks).interrupt = Some(strided);
            }
        })
    }
}
