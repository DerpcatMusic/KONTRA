# Spike P3.1: real-time-safe Luau allocator

Issue #703 · Task P3.1 · Spike code: `tools/spikes/luau-alloc/` (standalone crate, not a workspace member)

## Verdict

- **mlua 0.12.2 public API: NO.** It has no custom-allocator hook for Luau. Every mlua state uses mlua's own allocator, which calls the Rust global allocator (system malloc). Its memory limit caps bytes but does not redirect allocation.
- **Raw `lua_newstate` through `mlua::ffi`: YES.** Luau routes every allocation through one `frealloc` callback. A raw state on a fixed arena had zero Rust-side allocations in every measured window, and call latency matched mlua within noise.
- **Chosen design:** a thin shim in `kontra-lua/src/alloc.rs` over the raw FFI. It owns the state and never wraps it in `mlua::Lua` (see Risks).

## Evidence

mlua 0.12.2 and mlua-sys 0.13.0 are the versions pinned in `Cargo.lock`. luau0-src is `0.22.0+luau740`. A fresh lock resolves `+luau744`, and mlua then fails to configure (`attempt to load a text chunk (mode is 'b')`), so the spike's lock must be seeded from the repo's.

| Fact | Location |
|---|---|
| mlua's Luau allocator is a Rust fn that calls `alloc::alloc` / `realloc` / `dealloc` | `mlua-0.12.2/src/memory.rs:87` (fn), `:137`, `:146`, `:104` |
| mlua installs that allocator at state creation | `mlua-0.12.2/src/state/raw.rs:140`, `:143` (falls back to `luaL_newstate` at `:147`) |
| The allocator `ud` is mlua's `MemoryState`, and mlua reads it back | `memory.rs:26` (`mlua_assert` on the `ud`), `state.rs:1116` (`set_memory_limit` writes into it), `util/error.rs:164`, `:214` (`relax_limit_with`) |
| The public `get_or_init_from_ptr` accepts a foreign state, but mlua still reads the foreign `ud` as its `MemoryState` | `state.rs:436` |
| mlua-sys exposes the raw constructor `lua_newstate(f, ud)` | `mlua-sys-0.13.0/src/luau/lua.rs:113` |
| Luau has no hidden allocation: all state, GC object and stack memory goes through `frealloc` | `luau0-src` `VM/src/lstate.h:194`, `VM/src/lstate.cpp:213`, `VM/include/lua.h:131`; the only direct `malloc` path is the default `l_alloc` in `VM/src/linit.cpp:130`, used only by `luaL_newstate` |

Consequence: a custom allocator needs the raw `lua_newstate` path. mlua's own API has no way to pass one in.

## Chosen API

```text
lua_newstate(arena_alloc, arena_ptr)       // mlua_sys, raw
luaL_openlibs / luau_load                  // raw; outside pcall, see Risks
lua_pcall(...)                             // every script entry point
lua_gc(LUA_GCSTEP, kb)                     // bounded collector slice
```

`arena_alloc` is an `unsafe extern "C" fn` with the `lua_Alloc` signature. `arena_ptr` is a `Box<Arena>` handed over with `Box::into_raw`. The arena is a power-of-two size-class allocator over one region reserved at prepare:

- classes from 16 B to 64 MiB (23 classes); a free list per class and a bump cursor;
- allocation pops a free list or bumps the cursor, so it never calls the system allocator;
- `realloc` returns the same pointer when the class does not change, otherwise allocates, copies `min(old, new)`, and frees the old block;
- a request that cannot be satisfied returns null, and Luau raises `not enough memory`.

## Numbers

Release build, `AMD Ryzen 7 7800X3D`, one run of the committed binary (`cargo run --release --manifest-path tools/spikes/luau-alloc/Cargo.toml`). Earlier runs of the same code agreed on the call and loop rows within about 5 %. The GC rows varied between runs (the cycle count in a 1000-step window was 0 or 1, and the mean moved by about 3×), so read them as order-of-magnitude. Allocation counts are exact.

"allocs/op" counts Rust global-allocator calls inside the timed window. It excludes setup such as the 64 MiB reservation and the compile.

| backend | case | ops | allocs/op | bytes/op | mean ns | p99 ns | max ns | note |
|---|---|---:|---:|---:|---:|---:|---:|---|
| mlua default | Rust→Luau call, arithmetic | 100000 | 0.000 | 0.0 | 75 | 110 | 22420 | |
| mlua default | Rust→Luau call, 2-slot table alloc | 100000 | 0.002 | 33.5 | 108 | 350 | 4990 | |
| mlua default | Luau loop, 1e5 table allocs | 100000 | 0.002 | 33.0 | 51 | – | – | VM only |
| mlua default | Luau loop, 1e5 coroutine create+resume | 100000 | 0.000 | 0.5 | 190 | – | – | VM only |
| mlua default | `LUA_GCSTEP` data=0 ×1000, 1e5-table heap | 1000 | 0.000 | 0.0 | 555 | 550 | 301416 | 1 cycle finished |
| mlua default | `LUA_GCSTEP` data=64 KiB ×1000, 1e5-table heap | 1000 | 0.000 | 0.0 | 15591 | 299896 | 315256 | 16 cycles finished |
| raw arena 64 MiB | Rust→Luau call, arithmetic | 100000 | 0.000 | 0.0 | 70 | 90 | 2890 | |
| raw arena 64 MiB | Rust→Luau call, 2-slot table alloc | 100000 | 0.000 | 0.0 | 110 | 350 | 5010 | |
| raw arena 64 MiB | Luau loop, 1e5 table allocs | 100000 | 0.000 | 0.0 | 53 | – | – | VM only |
| raw arena 64 MiB | Luau loop, 1e5 coroutine create+resume | 100000 | 0.000 | 0.0 | 198 | – | – | VM only |
| raw arena 64 MiB | `LUA_GCSTEP` data=0 ×1000, 1e5-table heap | 1000 | 0.000 | 0.0 | 568 | 550 | 311716 | 1 cycle finished |
| raw arena 64 MiB | `LUA_GCSTEP` data=64 KiB ×1000, 1e5-table heap | 1000 | 0.000 | 0.0 | 15641 | 301585 | 319366 | 16 cycles finished |

Arena after the suite: reserved 11.8 MB, peak live 9.7 MB, 0 failures. Reserved over peak live is about 1.2×; that is the power-of-two rounding and the bump-only growth on this workload.

Exhaustion probe: a 4 MiB arena cannot hold the 1e5-table heap. `build` returns `Luau error: not enough memory` through `pcall`. The process does not abort.

Probes on the 256 KiB arena (a temporary run, not committed): the process aborts inside `RawVm::new` (`fatal runtime error: Rust cannot catch foreign exceptions`), before any Lua runs. See Risks.

## Risks

1. **An mlua `Lua` must never wrap an arena state.** mlua reads the allocator `ud` as its own `MemoryState` (memory.rs:26, util/error.rs:164). `set_memory_limit` and `pushcfunction` would write into the arena. The shim owns the raw state; no `mlua::Lua` handle is created for it.
2. **Unprotected Luau calls abort the process on out-of-memory.** `luaL_openlibs` and `luau_load` run outside `pcall`. A throw there unwinds through Rust, and Rust aborts. Observed at 256 KiB. Mitigation: prepare-time sizing with a headroom check before those calls, and refuse to load a script when headroom is short. Every script entry point goes through `pcall`.
3. **GC cycle-boundary pauses are about 0.3 ms on a 1e5-table heap.** Both backends show this at both step sizes. The worst step is not controlled by the data argument. Each step's max is about 0.3 ms, so one step per callback boundary is not enough on its own. The bound has to come from heap size: a per-script arena capacity plus a measured budget on the real UVI corpus. This synthetic heap is adversarial, and the real corpus must be measured before sign-off (`ONE_ENGINE.md` requires it).
4. **Size classes were too small at first.** A 64 KiB cap failed the 1e5-table build, since Luau doubles the array part to 2 MiB. The cap is now 64 MiB. Larger requests still fail and Luau raises a memory error.
5. **Power-of-two classes never coalesce.** Up to 2× internal waste per block, and free blocks of one class cannot serve another. The 1.2× result is one workload. Fragmentation under churn is not measured here.
6. **Pushing an `i64` gives Luau an integer subtype** that does not mix with numbers: `i * 2` raised `attempt to perform arithmetic (mul) on integer and number`. The boundary must push `f64`.
7. **Max latencies include OS noise.** Max values in the table come from scheduling and page faults on first touch of the arena. The p99 column is the number to judge.
8. **Setup differs.** The raw arena opens all of `luaL_openlibs`, mlua opens the sampler's subset. Per-call costs are unaffected, but the baseline memory is not the same.
9. **A Luau state is single-owner.** The arena and state are created and used on one thread (the audio thread, after prepare). They are not `Send` or `Sync`.

## Recommended design for 04-SCRIPT.md §4 `alloc.rs`

Follows the BeautifulRust skill (§8: `// SAFETY:` on every `unsafe` block, `# Safety` on every `unsafe fn`, `unsafe_op_in_unsafe_fn`).

```rust
/// Fixed-capacity size-class allocator for one Luau state. Memory is reserved in `new`.
#[derive(Debug)]
pub(crate) struct Arena { /* base, layout, bump, free[], live, peak_live */ }

/// `lua_Alloc` for a state created with `Arena` as `ud`.
///
/// # Safety
/// `ud` is the live `*mut Arena` given to `lua_newstate`; `ptr` is null or a block of size `osize` from it.
pub(crate) unsafe extern "C" fn luau_alloc(ud: *mut c_void, ptr: *mut c_void, osize: usize, nsize: usize) -> *mut c_void;

/// Owns one raw Luau state and its arena. Drop closes the state before the arena is freed.
#[derive(Debug)]
pub(crate) struct LuauHeap { state: *mut lua_State, arena: *mut Arena }
```

- `LuauHeap::new(capacity)` reserves the arena, runs the headroom check, then `lua_newstate` and `luaL_openlibs`. It returns `Result`; it does not panic on a memory shortage.
- Script entry points: `load` (`luau_load` then `pcall`), `call` (`lua_pcall`), `gc_step(kb)` (`LUA_GCSTEP`). Each returns `Result`, with the error message copied out before the stack is popped.
- Arena capacity is the memory limit. It replaces `set_memory_limit`, which does not apply to this path.
- Push numbers as `f64`. Integers stay out of the Luau boundary.
- The arena is created on the thread that owns the instance, and only that thread touches it.
- Tests: the exhaustion probe as a unit test (expects an `Err`, not an abort), and a `miri` run over the free-list code.

Open items before P3.2: measure GC cycle-boundary cost on the real UVI corpus; choose the per-script arena size from that measurement; decide the prepare-time headroom rule (the number is a guess until measured).

## Gate

- `tools/spikes/luau-alloc` builds and runs in release (`build-slot cargo run --release --manifest-path tools/spikes/luau-alloc/Cargo.toml`).
- `build-slot cargo fmt --check` and `build-slot cargo clippy --all-targets -- -D warnings` pass on the spike crate.
