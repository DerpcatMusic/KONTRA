//! GC step budgeting for the audio-safe path: the caller bounds collector work
//! per call instead of letting one `collectgarbage` pause the thread.

use mlua::Lua;

/// Run at most `max_steps` collector steps. Returns `true` when a full cycle
/// has finished. The collector is incremental under Luau's defaults, so each
/// step is one bounded slice of marking or sweeping.
pub fn bounded_gc_step(lua: &Lua, max_steps: usize) -> mlua::Result<bool> {
    for _ in 0..max_steps {
        if lua.gc_step()? {
            return Ok(true);
        }
    }
    Ok(false)
}
