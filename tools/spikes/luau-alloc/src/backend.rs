//! Two Luau backends behind one trait, so the same benchmark suite runs on both.
//!
//! [`MluaBackend`] uses mlua's default allocator. [`ArenaBackend`] drives a raw
//! `lua_newstate` whose allocator is [`crate::arena::luau_alloc`]. It must not be
//! wrapped in `mlua::Lua`: mlua reads the allocator `ud` as its own `MemoryState`
//! (mlua-0.12.2 src/memory.rs:26 and src/util/error.rs:164), which would be unsound here.

use std::ffi::{CStr, CString, c_void};
use std::ptr;

use mlua::{LuaOptions, StdLib, ffi};

use crate::arena::{Arena, luau_alloc};

/// Handle to a Luau global function, resolved once outside the timed regions.
#[derive(Clone, Copy, Debug)]
pub struct Func(usize);

pub trait Backend {
    fn resolve(&mut self, name: &str) -> Func;
    /// Calls a global Luau function with one number and returns its number result.
    fn call_num(&mut self, f: Func, arg: f64) -> Result<f64, String>;
    /// One bounded collector step. `kb` is Luau's work credit in KiB (`LUA_GCSTEP` data).
    fn gc_step(&mut self, kb: i32) -> Result<bool, String>;
}

#[derive(Debug)]
pub struct MluaBackend {
    lua: mlua::Lua,
    fns: Vec<mlua::Function>,
}

impl MluaBackend {
    /// Opens a state with the same std libraries the sampler uses and loads `source`.
    ///
    /// # Panics
    /// If mlua cannot create the state or the chunk fails to load.
    pub fn open(source: &str) -> Self {
        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE | StdLib::BIT;
        let lua = mlua::Lua::new_with(libs, LuaOptions::new()).expect("create mlua state");
        lua.load(source).exec().expect("load bench chunk");
        Self {
            lua,
            fns: Vec::new(),
        }
    }
}

impl Backend for MluaBackend {
    fn resolve(&mut self, name: &str) -> Func {
        let f: mlua::Function = self
            .lua
            .globals()
            .get(name)
            .unwrap_or_else(|e| panic!("global {name}: {e}"));
        self.fns.push(f);
        Func(self.fns.len() - 1)
    }

    fn call_num(&mut self, f: Func, arg: f64) -> Result<f64, String> {
        self.fns[f.0].call::<f64>(arg).map_err(|e| e.to_string())
    }

    fn gc_step(&mut self, kb: i32) -> Result<bool, String> {
        // SAFETY: `state()` is the live main state of `self.lua`. `LUA_GCSTEP` only advances
        // the collector and does not run Lua code.
        Ok(unsafe { ffi::lua_gc(self.lua.state(), ffi::LUA_GCSTEP, kb) } != 0)
    }
}

#[derive(Debug)]
pub struct ArenaBackend {
    vm: RawVm,
    names: Vec<CString>,
}

impl ArenaBackend {
    /// Creates a raw state on a `capacity`-byte arena and loads Luau bytecode `chunk`.
    ///
    /// # Errors
    /// If the state cannot be created in `capacity` bytes or the chunk fails to load.
    pub fn open(capacity: usize, chunk: &[u8]) -> Result<Self, String> {
        Ok(Self {
            vm: RawVm::new(capacity, chunk)?,
            names: Vec::new(),
        })
    }

    pub fn vm(&self) -> &RawVm {
        &self.vm
    }
}

impl Backend for ArenaBackend {
    fn resolve(&mut self, name: &str) -> Func {
        self.names
            .push(CString::new(name).expect("bench names contain no NUL"));
        Func(self.names.len() - 1)
    }

    fn call_num(&mut self, f: Func, arg: f64) -> Result<f64, String> {
        self.vm.call_global(&self.names[f.0], arg)
    }

    fn gc_step(&mut self, kb: i32) -> Result<bool, String> {
        Ok(self.vm.gc_step(kb))
    }
}

/// A raw Luau state whose memory comes only from its [`Arena`].
#[derive(Debug)]
pub struct RawVm {
    state: *mut ffi::lua_State,
    arena: *mut Arena,
}

impl RawVm {
    fn new(capacity: usize, chunk: &[u8]) -> Result<Self, String> {
        let arena = Box::into_raw(Arena::new(capacity));
        // SAFETY: `luau_alloc` matches `lua_Alloc`. `arena` is a live, unaliased `Arena`
        // that outlives the state, because `Drop` closes the state before freeing it.
        let state = unsafe { ffi::lua_newstate(luau_alloc, arena.cast::<c_void>()) };
        if state.is_null() {
            // SAFETY: `arena` came from `Box::into_raw` above and no state references it.
            unsafe { drop(Box::from_raw(arena)) };
            return Err(format!("lua_newstate failed in a {capacity} byte arena"));
        }
        // SAFETY: `state` is a fresh, valid state with the arena allocator.
        unsafe { ffi::luaL_openlibs(state) };
        let mut vm = Self { state, arena };
        vm.load(chunk)?;
        Ok(vm)
    }

    pub fn arena(&self) -> &Arena {
        // SAFETY: `arena` stays allocated until `Drop`, and Luau is not running while `&self`
        // is borrowed, so no `&mut Arena` is live.
        unsafe { &*self.arena }
    }

    fn load(&mut self, chunk: &[u8]) -> Result<(), String> {
        // SAFETY: `chunk` is valid bytecode of the given length and the name is a static,
        // NUL-terminated string. The state is live.
        let rc = unsafe {
            ffi::luau_load(
                self.state,
                c"bench".as_ptr(),
                chunk.as_ptr().cast(),
                chunk.len(),
                0,
            )
        };
        if rc != 0 {
            return Err(self.pop_error());
        }
        self.pcall(0, 0)
    }

    fn pcall(&mut self, nargs: i32, nresults: i32) -> Result<(), String> {
        // SAFETY: the function and its `nargs` arguments are on the stack. Handler index 0
        // means no message handler.
        let rc = unsafe { ffi::lua_pcall(self.state, nargs, nresults, 0) };
        if rc == 0 {
            Ok(())
        } else {
            Err(self.pop_error())
        }
    }

    /// Copies and pops the error object on top of the stack.
    fn pop_error(&mut self) -> String {
        // SAFETY: after a failed load or call an error value is on top. The string pointer is
        // copied before the pop that invalidates it.
        unsafe {
            let p = ffi::lua_tolstring(self.state, -1, ptr::null_mut());
            let text = if p.is_null() {
                "unknown Luau error".to_owned()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            };
            ffi::lua_settop(self.state, -2);
            text
        }
    }

    fn call_global(&mut self, name: &CStr, arg: f64) -> Result<f64, String> {
        let s = self.state;
        // SAFETY: `name` is NUL-terminated and alive for the call. `lua_getglobal` pushes one
        // value. The stack has room for the callee and one argument (LUA_MINSTACK).
        unsafe { ffi::lua_getglobal(s, name.as_ptr()) };
        // SAFETY: pushes one number onto the stack, which has room for it.
        unsafe { ffi::lua_pushnumber(s, arg) };
        self.pcall(1, 1)?;
        // SAFETY: a successful call with one result leaves that result on top of the stack.
        let value = unsafe { ffi::lua_tonumberx(s, -1, ptr::null_mut()) };
        // SAFETY: pops the one result pushed by the call above.
        unsafe { ffi::lua_settop(s, -2) };
        Ok(value)
    }

    fn gc_step(&mut self, kb: i32) -> bool {
        // SAFETY: `LUA_GCSTEP` only advances the collector on a live state and does not run Lua code.
        unsafe { ffi::lua_gc(self.state, ffi::LUA_GCSTEP, kb) != 0 }
    }
}

impl Drop for RawVm {
    fn drop(&mut self) {
        // SAFETY: `state` is live and closed exactly once, here, before its arena is freed.
        unsafe { ffi::lua_close(self.state) };
        // SAFETY: `arena` came from `Box::into_raw` in `new`. No state references it now, and
        // it is freed exactly once.
        unsafe { drop(Box::from_raw(self.arena)) };
    }
}
