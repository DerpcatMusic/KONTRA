//! VM creation and compiler policy. The library set is what UVI already grants:
//! base functions, table, string, math, coroutine and bit. No io, os, package,
//! debug, or dofile/loadfile.

use mlua::{Lua, LuaOptions, StdLib, chunk::Compiler};

/// Libraries a script may use. Adding one is a review item, not a default.
pub fn libs() -> StdLib {
    StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE | StdLib::BIT
}

/// Native code generation for a VM. Codegen runs inside every `load`, so it
/// is paid per chunk, per VM: 2-20 ms per UVI module (see
/// docs/architecture-v2/UNIVERSAL_SCRIPT.md). The interpreter is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Jit {
    #[default]
    Interpreter,
    Native,
}

/// A sandboxed Luau state with `memory` bytes allowed, interpreted.
pub fn new_vm(memory: usize) -> mlua::Result<Lua> {
    new_vm_with(memory, Jit::Interpreter)
}

/// A sandboxed Luau state with `memory` bytes allowed and the given codegen policy.
/// Affects chunks loaded after this call only.
pub fn new_vm_with(memory: usize, jit: Jit) -> mlua::Result<Lua> {
    let lua = Lua::new_with(libs(), LuaOptions::new())?;
    lua.set_memory_limit(memory)?;
    #[cfg(not(target_arch = "wasm32"))]
    lua.enable_jit(jit == Jit::Native);
    #[cfg(target_arch = "wasm32")]
    let _ = jit;
    Ok(lua)
}

/// The one compiler policy every front end uses: Luau defaults (optimization
/// level 1, debug level 1). Coverage is an inspector-only opt-in, applied by
/// the caller with `Compiler::set_coverage_level`.
pub fn compiler() -> Compiler {
    Compiler::default()
}

/// Compile-only validation. Runs no code and discards the bytecode.
pub fn check_syntax(source: &str) -> mlua::Result<()> {
    compiler().compile(source).map(drop)
}

/// Luau bytecode for `source`, for caching next to the script. Loading the
/// bytes skips the compiler: about 12x less load time with the interpreter
/// (see docs/architecture-v2/UNIVERSAL_SCRIPT.md). Bytecode is tied to the
/// Luau version; a cache must be keyed by the script hash and this crate's version.
pub fn compile_bytecode(source: &str) -> mlua::Result<Vec<u8>> {
    compiler().compile(source)
}
