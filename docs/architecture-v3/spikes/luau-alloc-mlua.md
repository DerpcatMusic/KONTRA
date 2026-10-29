# Spike P3.1b: keep mlua's API on an arena, and measure GC on real UVI scripts

Task P3.1b · Follows [P3.1](luau-alloc.md) · Spike code: `tools/spikes/luau-alloc-mlua/` (standalone crate, not a workspace member)

## Verdict

- **Recommend A: a routing `#[global_allocator]` with a per-thread scope flag.** It keeps mlua's high-level API, so none of the 28 mlua-using files change. Inside `ArenaScope`, every allocation mlua makes for Luau and every Rust allocation in a callback comes from one fixed arena. Measured: **0 system allocations inside scopes** across the synthetic suite and all five real hosts. Cost outside scopes: about **+1.1 ns per 48-byte alloc+free** (4.1 → 5.2 ns).
- **Fallback B: a 53-line patch to mlua 0.12.2** (`patches/mlua-0.12.2-luau-backing.patch`). It sends one state's Luau allocations to a `lua_Alloc`, with no global allocator. It is compile-checked and forwards Luau allocations correctly. It does **not** cover Rust allocations inside callbacks, and it needs a vendored mlua fork. Use it only if A's process-wide allocator is rejected (see Risks 1).
- **C (raw FFI) is not recommended.** The 28 files hold 219 mlua lines. A raw shim is 800–1,200 new lines (estimate, not built), and every call site is rewritten. P3.1 already showed it works, but A gets the same zero-allocation result for a fraction of the change.
- **The allocator is not the blocker for P3.2. GC pauses are.** On a 4e6-table heap (357 MB, the size of the Augmented Orchestra hosts), a collector cycle boundary stalls for **14.5–16 ms**. That is longer than any audio buffer. The 1e5-table heap (about 9 MB, the size of the VWinds hosts) stalls for about **0.3 ms**. The per-script heap budget below follows from this.

## Evidence

### Option A: routing allocator (`src/routed.rs`)

| Fact | Where |
|---|---|
| mlua's Luau allocator calls the Rust global allocator (`alloc::alloc`, `alloc::realloc`, `alloc::dealloc`) | `mlua-0.12.2/src/memory.rs:137`, `:146`, `:104` (from P3.1) |
| Frees route by address: a pointer inside the reserved region goes back to the arena whatever thread frees it, otherwise to `System` | `Routed::dealloc`, `Arena::owns` |
| Allocations are arena-served only while `IN_SCOPE` is set on the calling thread. The flag is a `const` thread-local `Cell<bool>`, so reading it never allocates | `ArenaScope::enter`, `routed.rs` |
| Blocks are power-of-two classes from 16 B to 64 MiB, and a block of class `c` is aligned to its own size. So alignment up to the block size is met without a separate path (the real hosts needed 128 B alignment) | `class_of`, `Arena::take` |
| Spills (scope allocations that do not fit) go to `System` and are counted as violations | `Stats::system_in_scope`, `ArenaStats::spills` |

Test: `tests/routing.rs` covers scope routing, off-scope free returning an arena block (same address reused), System block freed in scope, spill counting, realloc moving a System block into the arena with bytes intact, 128-byte alignment, and a cross-thread free. It passes.

### Option B: mlua patch (`patches/mlua-0.12.2-luau-backing.patch`)

- `memory.rs`: `MemoryState` gains `backing: Option<(lua_Alloc, *mut c_void)>`. `allocator()` forwards to it when set. About 20 lines with comments.
- `state.rs`: `Lua::set_luau_backing(backing, ud)` (`unsafe`). About 11 lines.
- Compile check: a scratch crate against the patched mlua (`luau0-src` pinned to +luau740 from the repo lock) ran a 1e5-table build through the backing with the correct result and 100,463 backing calls. The patch applies cleanly to a fresh mlua 0.12.2.
- Not benchmarked. The check is functional only.

### Option C: raw FFI (from P3.1)

| Measure | Value |
|---|---|
| Files that mention mlua | 28 (`rg -l mlua --type rust crates src`) |
| Lines that mention mlua | 219 (the eight largest files hold 159 of them: 40 + 37 + 20 + 18 + 14 + 11 + 10 + 9) |
| Mentions in the sampler-uvi script host | about 130, in `script.rs` and `script/*.rs` |
| Size of the P3.1 raw shim | `arena.rs` 181 lines, `backend.rs` 215 lines |
| New lines to replace mlua's API (estimate) | 800–1,200, plus rewrites of the 219 call sites |

The estimate is from the shim size and the API surface. It was not built.

## Numbers

Release builds on the same AMD Ryzen 7 7800X3D as P3.1, `build-slot cargo run --release`. Each table is one run. The GC rows and max columns vary between runs by 30–100 % (the same cycle-boundary row read 0.27 ms and 1.9 ms in two runs), so read them as order-of-magnitude. Allocation counts are exact.

### Synthetic suite (P3.1 cases, mlua API, `src/main.rs`)

"system-in-scope/op" counts allocations inside a scope that went to `System`. Zero is the target.

| backend | case | allocs/op | arena/op | system-in-scope/op | mean ns | p99 ns | max ns | note |
|---|---|---:|---:|---:|---:|---:|---:|---|
| mlua, no scope | Rust→Luau call, arithmetic | 0.000 | 0.000 | 0.000 | 70 | 100 | 16280 | |
| mlua + arena scope (A) | Rust→Luau call, arithmetic | 0.000 | 0.000 | 0.000 | 70 | 90 | 9480 | |
| mlua + arena scope (A) | Rust→Luau call, 2-slot table alloc | 0.002 | 0.002 | 0.000 | 107 | 350 | 52611 | max is first-touch noise |
| mlua + arena scope (A) | Luau loop, 1e5 table allocs | 0.002 | 0.002 | 0.000 | 44 | – | – | |
| mlua + arena scope (A) | Luau loop, 1e5 coroutine create+resume | 0.000 | 0.000 | 0.000 | 184 | – | – | |
| mlua, no scope | `LUA_GCSTEP` data=64 KiB ×1000, 1e5-table heap | – | – | – | 15333 | 289995 | 306346 | 16 cycles finished |
| mlua + arena scope (A) | `LUA_GCSTEP` data=64 KiB ×1000, 1e5-table heap | – | – | 0.000 | 15427 | 302195 | 308405 | 16 cycles finished |
| mlua + arena scope (A) | `LUA_GCSTEP` data=0 ×1000, 1e5-table heap | – | – | 0.000 | 190 | 330 | 3960 | 0 cycles finished |
| mlua + arena scope (A), 4e6-table heap | `LUA_GCSTEP` data=64 KiB ×10000 | – | – | 0.000 | 21807 | 27661 | **14490765** | **4 cycles finished** |
| mlua + arena scope (A), 4e6-table heap | `LUA_GCSTEP` data=0 ×10000 | – | – | 0.000 | 220 | 490 | 5491 | 0 cycles finished |

The 4e6-table heap built in the arena peaks at 357,696 KiB. The arena is 1 GiB. The 1e5-table suite peaks at 9.3 MB, and the arena stats after the suite show 0 spills.

### Allocator overhead (flag check on ordinary allocations)

| allocator path | ns per alloc+free (48 B) |
|---|---:|
| `System` direct (no wrapper) | 4.12 |
| `Routed`, no scope (flag check, TLS counters, address check) | 5.18 |
| `Routed`, in scope (arena block, spin lock taken) | 4.94 |

So the flag check and the counters add about 1.1 ns per pair on the non-audio path. On the scope path, the arena is about as fast as glibc.

### Real UVI programs (`src/bin/uvi_host.rs`)

Each program is run through the sampler's own `ScriptHost::new_profiled` on the bench thread, inside `ArenaScope`. Heap is the host's `lua_memory_bytes` after init. Init includes the host's profiling overhead.

| program | bank members | host init | init ms | Lua heap KiB | arena peak KiB | arena allocs | system-in-scope |
|---|---:|---|---:|---:|---:|---:|---:|
| VWinds-AClarinet / `Presets/Clarinet A.uvip` | 4 | ok | 46.5 | 8,814 | 28,597 | 180,577 | 0 |
| VWinds-Alto_Flute / `Presets/Alto Flute.uvip` | 5 | ok | 57.7 | 9,695 | 32,880 | 233,451 | 0 |
| VWinds-Bassoon / `Presets/Bassoon.uvip` | 5 | ok | 46.9 | 8,475 | 28,401 | 176,046 | 0 |
| Augmented Orchestra / `Presets/02 Action/Waltz Night 1.uvip` | 19 | ok | 2,009 | 379,377 | 569,381 | 3,986,131 | 0 |
| Augmented Orchestra / `Presets/10 Augmented Layers/Modeled/Mod Bright Horns.uvip` | 19 | ok | 2,056 | 354,763 | 545,964 | 4,058,704 | 0 |

The arena reserved 775 MB across the five programs, so reserved/peak is 1.36 on the largest. The three VWinds programs are in separate banks with separate scripts. The two Augmented Orchestra programs share one script set.

**Not measured:** GC step cost on real scripts. The host's Lua state is private, and no public API steps its collector or runs a note through it. The note stream is therefore not timed either. Real-host GC is the open item below.

## Per-script arena rule

`arena_bytes = max(64 MiB, next_power_of_two(1.5 × peak_live))`, where `peak_live` is the arena peak over init and the note stream.

- VWinds-class hosts: peak 28–33 MB, so **64 MiB**.
- Augmented Orchestra-class hosts: peak 546–569 MB, so **1 GiB**.

The 1.5 margin covers the power-of-two rounding and the alignment gaps. Measured reserved/peak is 1.36 on the real hosts and 1.39 on the synthetic suite. The peak here is for init only. The note-stream term needs the hook listed under Open items.

A spill inside a scope is a bug, not a slowdown. A spill is counted, and the test gate should fail on any nonzero count.

## GC step budget

Bounded `LUA_GCSTEP` steps are cheap. The cycle boundary is not bounded by the step size.

| heap | bounded step, p99 | cycle boundary, max | cycles per 10k steps (kb=64) |
|---|---:|---:|---:|
| 1e5 tables, about 9 MB | 0.3 ms | 0.3 ms | 16 per 1000 steps |
| 4e6 tables, about 357 MB | 27.7 µs | **14.5–16 ms** | 4 per 10000 steps |

Proposed budget for P3.2:

- **Per callback, GC work: p99 at most 50 µs.** Use `LUA_GCSTEP` with a small `kb`.
- **Cycle boundary: at most 0.5 ms.** At this size a heap of about 10 MB keeps the boundary near the 0.3 ms measured for the 1e5-table heap.
- **Per-script live heap cap: about 10 MB** for audio-thread callbacks, enforced by `set_memory_limit` (mlua path) or by the arena size (with A, a spill is a fail).
- **Augmented Orchestra-class hosts (355–380 MB)** exceed the cap by about 35×. A cycle boundary at that size is 14–16 ms, so these scripts cannot meet the budget on the audio thread with the current Luau collector. The heap composition is not decomposed here. Whether most of the 355–380 MB is tables, strings or the XML-derived program model decides the fix. P3.2 has to choose one of: shrink the heap, move collection off the audio thread (the state is single-owner, so this needs a hand-off), or keep these programs on the `ScriptThread` fallback.

## Risks

1. **A `#[global_allocator]` is process-wide.** It is safe in this crate's binaries, which are the only users. In a plugin loaded by a host process, another Rust dylib with its own global allocator would conflict. This was not tested with two cdylibs. Check that before P3.2, or switch to B (B has no global allocator, and its upkeep is a mlua fork).
2. **Frees of arena blocks from another thread take a spin lock.** The lock is held only for a free-list operation, with no syscalls. But a preempted lock holder can stall the audio thread, so this is not wait-free. A deferred-free queue drained on the audio thread would remove the lock from that path. Not built.
3. **Spills.** Any spill goes to `System` inside the scope. The counter catches it. Size the arena by the rule above.
4. **Power-of-two classes never coalesce.** Reserved/peak was 1.36–1.39 on this workload. Fragmentation under churn is not measured.
5. **Luau JIT.** `sampler-uvi` enables `luau-jit` on native targets, so this spike's mlua also has it. JIT code pages are allocated outside Luau's `frealloc`, so they are not in the arena. Not measured.
6. **Rust-side allocations inside a scope are arena-served under A.** This is good for the zero-allocation goal, but a large Rust allocation (over 64 MiB) inside a callback goes to `System` and is counted. Under B this case is not covered at all.
7. **Integers.** Carried over from P3.1: push `f64` across the boundary.
8. **Unprotected memory errors abort.** Carried over from P3.1: `luaL_openlibs` and `luau_load` outside `pcall`. The mlua path has `pcall` through the normal `Result` API.

## Open items before P3.2

- A hook in `sampler-uvi` to step the host's collector and run one note. Without it, real-script GC cost stays unmeasured. This is a one-function change outside this spike's scope.
- A decision on the Augmented Orchestra heap (see GC budget).
- The two-cdylib test for Risk 1.

## Deviations from the brief

- **Base.** PR #773 has merged, so this branch starts from `origin/v3` (`f56e2767d`), not from the P3.1 branch. No rebase was needed.
- **Real scripts: heap and arena, not GC.** The brief asked for heap size and worst GC step per script. The sampler's `ScriptHost` runs the bank's scripts through its own mlua state, which is private, so only the post-init heap and the init-time arena peak are measured. An attempt to run the scripts through a standalone harness with the sampler's own prelude failed at init. The scripts need host objects (for example `Program.layers[i].keygroups`) that the harness does not model, so that attempt was dropped and is not in the tree.
- **Five programs, four script sets.** The five programs are from three VWinds banks and one Augmented Orchestra bank. The two Augmented Orchestra programs share one script set.
- **Option B is functional only.** It is compile-checked and forwards allocations, but it was not run through the benchmark suite. Its patch and compile check are in the spike directory.
- **`ScriptHost::new_profiled` is used for init.** It adds profiling overhead to the init times. The heap numbers are unaffected.

## Gate

- `build-slot cargo run --release --manifest-path tools/spikes/luau-alloc-mlua/Cargo.toml --bin luau-alloc-mlua` and `--bin uvi_host`: built and run in release with the numbers above.
- `build-slot cargo test --manifest-path tools/spikes/luau-alloc-mlua/Cargo.toml`: routing test passes.
- `cargo fmt --check` and `build-slot cargo clippy --all-targets -- -D warnings` on the spike crate: clean.
- `python3 tools/plan/plan.py scope P3.1b --base origin/v3`: `scope P3.1b: ok`.
