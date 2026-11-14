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
    /// Safepoints until the next call of mlua's interrupt.
    static LEFT: Cell<u32> = const { Cell::new(STRIDE) };
    static EVERY: Cell<u32> = const { Cell::new(STRIDE) };
    static PAST: Cell<bool> = const { Cell::new(false) };
}

/// Safepoints per call of mlua's interrupt from now on. A run that must
/// yield checks often at first (one host call can take tens of
/// microseconds), then at every safepoint once its slice is spent: Luau can
/// yield only at some of them (a loop edge, not inside a C call), and a loop
/// with a fixed count of safepoints per turn would otherwise keep landing
/// on the others.
pub(crate) fn set_stride(every: u32) {
    EVERY.with(|e| e.set(every.max(1)));
    LEFT.with(|l| l.set(every.max(1)));
}

/// The run's slice is spent: from now on only the safepoints Luau can yield
/// at reach mlua's interrupt. The pattern matcher (`string.find`, `gmatch`)
/// fires the interrupt at every step, inside a C call that cannot yield; a
/// long match is hundreds of thousands of refused requests, and each one that
/// ran the callback cost more than the match itself.
pub(crate) fn set_past(past: bool) {
    PAST.with(|p| p.set(past));
    if past {
        LEFT.with(|l| l.set(1));
    }
}

/// What each call of mlua's interrupt stands for now.
pub(crate) fn stride() -> u32 {
    EVERY.with(Cell::get)
}

unsafe extern "C-unwind" fn strided(state: *mut ffi::lua_State, gc: c_int) {
    if gc >= 0 {
        crate::probe::gc_edge();
    }
    // The pattern matcher fires this at every step: the common case is one
    // counter, down by one.
    let left = LEFT.with(Cell::get);
    if left > 1 {
        LEFT.with(|l| l.set(left - 1));
        return;
    }
    let past = PAST.with(Cell::get);
    // SAFETY: `state` is the running thread, as Luau passes it.
    if past && unsafe { ffi::lua_isyieldable(state) } == 0 {
        return;
    }
    LEFT.with(|l| l.set(if past { 1 } else { stride() }));
    if let Some(hook) = MLUA.get() {
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
